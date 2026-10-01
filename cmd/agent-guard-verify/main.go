package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"regexp"
	"strings"
	"sync/atomic"
	"syscall"
	"time"
)

const usage = "usage: agent-guard-verify [absolute-installed-executable]"
const deadline = 4500 * time.Millisecond

func main() { os.Exit(runMain(os.Args[1:])) }

func runMain(args []string) int {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	interrupts := make(chan os.Signal, 1)
	signal.Notify(interrupts, syscall.SIGINT, syscall.SIGTERM, syscall.SIGHUP)
	defer signal.Stop(interrupts)
	var interrupted atomic.Int32
	go func() {
		select {
		case s := <-interrupts:
			interrupted.Store(128 + int32(s.(syscall.Signal)))
			cancel()
		case <-ctx.Done():
		}
	}()
	code := verify(ctx, args, os.Stdout, os.Stderr)
	if code := interrupted.Load(); code != 0 {
		return int(code)
	}
	return code
}

type installation struct {
	entry, native, version string
	entryHash, nativeHash  [32]byte
}

func installed(args []string) (installation, error) {
	var pkg installation
	if len(args) > 1 {
		return pkg, errors.New(usage)
	}
	var path string
	var err error
	if len(args) == 1 {
		path = args[0]
	} else {
		path, err = exec.LookPath("agent-guard")
		if err != nil {
			return pkg, fmt.Errorf("missing installed executable: %w", err)
		}
	}
	if !filepath.IsAbs(path) {
		return pkg, errors.New("missing absolute installed executable; " + usage)
	}
	pkg.entry, err = filepath.EvalSymlinks(path)
	if err != nil {
		return pkg, err
	}
	if err := executable(pkg.entry); err != nil {
		return pkg, err
	}
	if err := outsideCheckout(filepath.Dir(pkg.entry)); err != nil {
		return pkg, fmt.Errorf("refusing a Git checkout executable: %w", err)
	}
	if filepath.Base(pkg.entry) != "agent-guard" || filepath.Base(filepath.Dir(pkg.entry)) != "bin" {
		return pkg, errors.New("selected entry is not the installed bin/agent-guard executable")
	}
	body, err := os.ReadFile(filepath.Join(filepath.Dir(pkg.entry), "../VERSION"))
	if err != nil {
		return pkg, err
	}
	pkg.version = strings.TrimSpace(string(body))
	if !regexp.MustCompile(`^0\.\d+\.\d+$`).MatchString(pkg.version) {
		return pkg, errors.New("installed VERSION is missing or invalid")
	}
	pkg.native, err = filepath.EvalSymlinks(filepath.Join(filepath.Dir(pkg.entry), "agent-guard-native"))
	if err != nil {
		return pkg, err
	}
	if err := executable(pkg.native); err != nil {
		return pkg, err
	}
	for _, file := range []struct {
		path string
		hash *[32]byte
	}{{pkg.entry, &pkg.entryHash}, {pkg.native, &pkg.nativeHash}} {
		body, err := os.ReadFile(file.path)
		if err != nil {
			return pkg, err
		}
		*file.hash = sha256.Sum256(body)
	}
	return pkg, nil
}

func executable(path string) error {
	info, err := os.Stat(path)
	if err != nil {
		return err
	}
	if !info.Mode().IsRegular() || info.Mode().Perm()&0111 == 0 {
		return fmt.Errorf("not an executable file: %s", path)
	}
	return nil
}

func outsideCheckout(path string) error {
	for directory := path; ; directory = filepath.Dir(directory) {
		if _, err := os.Lstat(filepath.Join(directory, ".git")); err == nil {
			return fmt.Errorf("%s contains .git", directory)
		} else if !errors.Is(err, os.ErrNotExist) {
			return err
		}
		if directory == filepath.Dir(directory) {
			return nil
		}
	}
}

type result struct {
	exit           int
	stdout, stderr string
	timedOut       bool
	signal         syscall.Signal
}

func execute(ctx context.Context, path string, args []string, body []byte, cwd string, env []string) (result, error) {
	ctx, cancel := context.WithTimeout(ctx, deadline)
	defer cancel()
	cmd := exec.CommandContext(ctx, path, args...)
	cmd.Dir, cmd.Env, cmd.Stdin = cwd, env, bytes.NewReader(body)
	cmd.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
	stop := func() error {
		err := syscall.Kill(-cmd.Process.Pid, syscall.SIGKILL)
		if errors.Is(err, syscall.ESRCH) {
			return nil
		}
		return err
	}
	cmd.Cancel = stop
	// Descendants holding output pipes must not keep Wait alive after cancellation.
	cmd.WaitDelay = 500 * time.Millisecond
	var stdout, stderr bytes.Buffer
	cmd.Stdout, cmd.Stderr = &stdout, &stderr
	if err := cmd.Start(); err != nil {
		return result{}, err
	}
	err := cmd.Wait()
	if cleanup := stop(); cleanup != nil {
		return result{}, cleanup
	}
	r := result{exit: cmd.ProcessState.ExitCode(), stdout: stdout.String(), stderr: stderr.String(), timedOut: errors.Is(ctx.Err(), context.DeadlineExceeded)}
	if status, ok := cmd.ProcessState.Sys().(syscall.WaitStatus); ok && status.Signaled() {
		r.signal = status.Signal()
	}
	var exit *exec.ExitError
	if err != nil && !errors.As(err, &exit) && !errors.Is(err, context.Canceled) && !errors.Is(err, context.DeadlineExceeded) {
		return r, err
	}
	return r, nil
}

type acceptanceCase struct {
	name, tool string
	input      map[string]string
	expected   int
	reason     string
	advice     string
}

func acceptanceCases(home, cwd string) []acceptanceCase {
	return []acceptanceCase{
		{name: "project-shell", tool: "Bash", input: map[string]string{"command": "cat file.txt"}},
		{name: "project-read", tool: "Read", input: map[string]string{"file_path": filepath.Join(cwd, "file.txt")}},
		{name: "environment-write", tool: "Write", input: map[string]string{"file_path": filepath.Join(cwd, ".env"), "content": ""}},
		{name: "client-key-use", tool: "Bash", input: map[string]string{"command": "ssh -i ~/.ssh/id_rsa example.invalid"}},
		{name: "public-key-read", tool: "Read", input: map[string]string{"file_path": filepath.Join(home, ".ssh/id.pub")}},
		{name: "project-search", tool: "Grep", input: map[string]string{"path": filepath.Join(cwd, "src"), "pattern": "canary"}},
		{name: "claude-advice", tool: "Bash", input: map[string]string{"command": "rg -rn canary src"}, advice: "rg -r means --replace. Drop -r; use -n for line numbers, or spell --replace VALUE for an intentional replacement."},
		{name: "appdata-shell", tool: "Bash", input: map[string]string{"command": "cat ~/Library/Containers/com.example.canary/file.txt"}, expected: 2, reason: "This reads a protected macOS app-data directory."},
		{name: "appdata-read", tool: "Read", input: map[string]string{"file_path": filepath.Join(home, "Library/Containers/com.example.canary/file.txt")}, expected: 2, reason: "This reads a protected macOS app-data directory."},
		{name: "appdata-link", tool: "Bash", input: map[string]string{"command": "cat data-link/com.example.canary/file.txt"}, expected: 2, reason: "This reads a protected macOS app-data directory."},
		{name: "environment-read", tool: "Bash", input: map[string]string{"command": "cat .env"}, expected: 2, reason: "This reads a credential or environment file."},
		{name: "private-key-read", tool: "Read", input: map[string]string{"file_path": filepath.Join(home, ".ssh/id_rsa")}, expected: 2, reason: "This reads a credential or environment file."},
		{name: "broad-scan-advice", tool: "Bash", input: map[string]string{"command": "rg -rn canary ~/Library"}, expected: 2, reason: "A scan rooted at the home directory or ~/Library"},
	}
}

func mismatches(r result, row acceptanceCase, runtime string) []string {
	var problems []string
	if r.timedOut {
		problems = append(problems, "deadline exceeded")
	}
	if r.signal != 0 {
		problems = append(problems, "signal "+r.signal.String())
	}
	if r.exit != row.expected {
		problems = append(problems, fmt.Sprintf("expected exit %d", row.expected))
	}
	if row.expected == 2 {
		if strings.TrimSpace(r.stderr) == "" {
			problems = append(problems, "empty denial reason")
		}
		if !strings.Contains(r.stderr, row.reason) {
			problems = append(problems, "wrong denial reason")
		}
		if runtime == "claude" && !strings.HasPrefix(r.stderr, "DENIED: ") {
			problems = append(problems, "missing Claude denial prefix")
		}
		if strings.TrimSpace(r.stdout) != "" {
			problems = append(problems, "denial emitted advice or unexpected stdout")
		}
	} else {
		if strings.TrimSpace(r.stderr) != "" {
			problems = append(problems, "unexpected stderr on allow")
		}
		if runtime == "claude" && row.advice != "" {
			var output struct {
				HookSpecificOutput struct{ HookEventName, AdditionalContext string }
			}
			if err := json.Unmarshal([]byte(r.stdout), &output); err != nil {
				problems = append(problems, "invalid Claude advice JSON")
			} else if output.HookSpecificOutput.HookEventName != "PreToolUse" || output.HookSpecificOutput.AdditionalContext != row.advice {
				problems = append(problems, "missing Claude advice")
			}
		} else if strings.TrimSpace(r.stdout) != "" {
			problems = append(problems, "unexpected advice on allow")
		}
	}
	return problems
}

func verify(ctx context.Context, args []string, stdout, stderr io.Writer) int {
	if err := verifyPackage(ctx, args, stdout); err != nil {
		fmt.Fprintf(stderr, "FAIL\t%s\n", err)
		return 1
	}
	return 0
}

func verifyPackage(ctx context.Context, args []string, stdout io.Writer) error {
	pkg, err := installed(args)
	if err != nil {
		return fmt.Errorf("setup\t%w", err)
	}
	fmt.Fprintf(stdout, "Executable: %s\nVersion: %s\nEntry SHA256: %x\nNative SHA256: %x\n", pkg.entry, pkg.version, pkg.entryHash, pkg.nativeHash)
	temporary, err := os.MkdirTemp("", "agent-guard-acceptance-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(temporary)
	temporary, err = filepath.EvalSymlinks(temporary)
	if err != nil {
		return err
	}
	home, cwd := filepath.Join(temporary, "home"), filepath.Join(temporary, "home/project")
	for _, directory := range []string{"home/.ssh", "home/Library/Containers/com.example.canary", "home/project/src"} {
		if err := os.MkdirAll(filepath.Join(temporary, directory), 0700); err != nil {
			return err
		}
	}
	for _, path := range []string{".ssh/id_rsa", ".ssh/id.pub", "project/.env", "project/file.txt", "Library/Containers/com.example.canary/file.txt"} {
		if err := os.WriteFile(filepath.Join(home, path), nil, 0600); err != nil {
			return err
		}
	}
	if err := os.Symlink(filepath.Join(home, "Library/Containers"), filepath.Join(cwd, "data-link")); err != nil {
		return err
	}
	env := []string{"HOME=" + home, "TMPDIR=" + temporary, "PATH=/usr/bin:/bin"}
	for _, path := range []string{pkg.native, pkg.entry} {
		r, err := execute(ctx, path, []string{"--version"}, nil, cwd, env)
		if err != nil {
			return err
		}
		if r.timedOut || r.signal != 0 || r.exit != 0 || r.stderr != "" || r.stdout != "agent-guard "+pkg.version+"\n" {
			return fmt.Errorf("installed version does not match VERSION: %s", path)
		}
	}
	rows := acceptanceCases(home, cwd)
	planned, completed, failures := len(rows)*2, 0, 0
	for _, row := range rows {
		if row.tool == "Bash" {
			planned++
		}
	}
	fmt.Fprintln(stdout, "RESULT\tRUNTIME\tCASE\tEXPECTED\tACTUAL\tREASON / MISMATCH")
	for _, runtime := range []string{"claude", "codex", "pi"} {
		for _, row := range rows {
			if runtime == "codex" && row.tool != "Bash" {
				continue
			}
			if err := ctx.Err(); err != nil {
				return err
			}
			tool := row.tool
			if runtime == "pi" {
				tool = strings.ToLower(tool)
			}
			event, err := json.Marshal(map[string]any{"tool_name": tool, "tool_input": row.input, "cwd": cwd})
			if err != nil {
				return err
			}
			r, err := execute(ctx, pkg.entry, []string{"--runtime", runtime}, event, cwd, env)
			if err != nil {
				return err
			}
			problems := mismatches(r, row, runtime)
			status := "PASS"
			if len(problems) != 0 {
				status = "FAIL"
				failures++
			}
			completed++
			detail := strings.TrimSpace(strings.Join(append([]string{r.stderr}, problems...), "; "))
			if detail == "" {
				detail = "-"
			}
			fmt.Fprintf(stdout, "%s\t%s\t%s\t%d\t%d\t%s\n", status, runtime, row.name, row.expected, r.exit, strings.Join(strings.Fields(detail), " "))
			if r.timedOut || ctx.Err() != nil {
				fmt.Fprintf(stdout, "Summary: %d pass, %d fail; %d/%d completed\n", completed-failures, failures, completed, planned)
				return errors.New("acceptance interrupted or deadline exceeded")
			}
		}
	}
	fmt.Fprintf(stdout, "Summary: %d pass, %d fail; %d/%d completed\n", completed-failures, failures, completed, planned)
	if failures != 0 {
		return errors.New("installed package acceptance failed")
	}
	return nil
}
