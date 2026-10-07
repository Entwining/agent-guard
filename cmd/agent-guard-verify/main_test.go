package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"syscall"
	"testing"
	"time"

	"agentguard/tests/harness"
)

func testPackage(t *testing.T, root, fault string) string {
	t.Helper()
	bin := filepath.Join(root, "package/bin")
	if err := os.MkdirAll(bin, 0700); err != nil {
		t.Fatal(err)
	}
	for path, body := range map[string]string{
		filepath.Join(root, "package/VERSION"):   "0.0.0\n",
		filepath.Join(bin, "agent-guard-native"): "#!/bin/sh\nprintf 'agent-guard 0.0.0\\n'\n",
	} {
		if err := os.WriteFile(path, []byte(body), 0700); err != nil {
			t.Fatal(err)
		}
	}
	entry := filepath.Join(bin, "agent-guard")
	body := "#!/bin/sh\nif [ \"$1\" = --version ]; then printf 'agent-guard 0.0.0\\n'; exit 0; fi\nexport GORACE=atexit_sleep_ms=0\nexec " + shellQuote(os.Args[0]) + " -test.run=^TestUtilityProcess$ -- fixture " + shellQuote(fault) + " \"$@\"\n"
	if err := os.WriteFile(entry, []byte(body), 0700); err != nil {
		t.Fatal(err)
	}
	return entry
}

func shellQuote(s string) string { return "'" + strings.ReplaceAll(s, "'", "'\\''") + "'" }

func runVerification(t *testing.T, entry string) (int, string, string) {
	t.Helper()
	var out, errout bytes.Buffer
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	code := verify(ctx, []string{entry}, &out, &errout)
	return code, out.String(), errout.String()
}

func protocolCases() map[string]int {
	want := map[string]int{}
	for _, runtime := range []string{"claude", "codex", "pi"} {
		for _, row := range acceptanceCases("/synthetic/home", "/synthetic/home/project") {
			if runtime != "codex" || row.tool == "Bash" {
				want[runtime+"\t"+row.name] = row.expected
			}
		}
	}
	return want
}

func assertProtocolCases(t *testing.T, output string) {
	t.Helper()
	want := protocolCases()
	if len(want) == 0 {
		t.Fatal("missing protocol case partition")
	}
	for _, runtime := range []string{"claude", "codex", "pi"} {
		found := false
		for key := range want {
			if strings.HasPrefix(key, runtime+"\t") {
				found = true
				break
			}
		}
		if !found {
			t.Fatalf("missing protocol case partition for %s", runtime)
		}
	}
	seen := map[string]bool{}
	for _, line := range strings.Split(output, "\n") {
		if !strings.HasPrefix(line, "PASS\t") && !strings.HasPrefix(line, "FAIL\t") {
			continue
		}
		fields := strings.SplitN(line, "\t", 6)
		if len(fields) != 6 {
			t.Fatalf("malformed protocol row: %q", line)
		}
		key := fields[1] + "\t" + fields[2]
		exit, exists := want[key]
		if !exists || seen[key] || fields[0] != "PASS" || fields[3] != strconv.Itoa(exit) || fields[4] != strconv.Itoa(exit) {
			t.Fatalf("missing, duplicate or mismatched protocol case: %q", line)
		}
		seen[key] = true
	}
	for key := range want {
		if !seen[key] {
			t.Fatalf("unevaluated protocol case: %s", key)
		}
	}
	if !strings.Contains(output, fmt.Sprintf("Summary: %d pass, 0 fail; %d/%d completed", len(want), len(want), len(want))) {
		t.Fatalf("wrong protocol summary: %s", output)
	}
}

// This assembles the protocol fixture independently of make build, whose packaging CI checks.
func TestInstalledPackage(t *testing.T) {
	root := t.TempDir()
	if err := os.Mkdir(filepath.Join(root, ".git"), 0700); err != nil {
		t.Fatal(err)
	}
	t.Setenv("TMPDIR", root)
	entry := testPackage(t, root, "none")
	if err := os.Remove(filepath.Join(filepath.Dir(entry), "agent-guard-native")); err != nil {
		t.Fatal(err)
	}
	_, file, _, _ := runtime.Caller(0)
	source := filepath.Clean(filepath.Join(filepath.Dir(file), "../.."))
	cargo := os.Getenv("CARGO")
	if cargo == "" {
		cargo = "cargo"
	}
	cargo, err := exec.LookPath(cargo)
	if err != nil {
		t.Fatal(err)
	}
	target := os.Getenv("CARGO_TARGET_DIR")
	if target == "" {
		target = t.TempDir()
	}
	if !filepath.IsAbs(target) {
		t.Fatal("CARGO_TARGET_DIR must be absolute")
	}
	target, err = harness.OutsidePath(target)
	if err != nil {
		t.Fatal(err)
	}
	// Compilation is setup; the Go test and CI job own its completion limit.
	build := exec.CommandContext(t.Context(), cargo, "build", "--locked", "--release", "--bin", "agent-guard-native", "--target-dir", target)
	build.Dir = source
	build.Env = os.Environ()
	build.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
	build.Cancel = func() error { return syscall.Kill(-build.Process.Pid, syscall.SIGKILL) }
	build.WaitDelay = time.Second
	if output, err := build.CombinedOutput(); err != nil {
		t.Fatalf("build native: %v: %s", err, output)
	} else {
		t.Logf("build native: %s", output)
	}
	if requested := os.Getenv("CARGO_TARGET_DIR"); requested != "" {
		if _, err := os.Stat(filepath.Join(requested, "release/agent-guard-native")); err != nil {
			t.Fatalf("caller target was not populated: %v", err)
		}
	}
	native, err := os.ReadFile(filepath.Join(target, "release/agent-guard-native"))
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(filepath.Dir(entry), "agent-guard-native"), native, 0700); err != nil {
		t.Fatal(err)
	}
	version, err := os.ReadFile(filepath.Join(source, "VERSION"))
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "package/VERSION"), version, 0600); err != nil {
		t.Fatal(err)
	}
	shell, err := os.ReadFile(filepath.Join(source, "bin/agent-guard"))
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(entry, shell, 0700); err != nil {
		t.Fatal(err)
	}
	code, out, errout := runVerification(t, entry)
	if code != 0 || errout != "" {
		t.Fatalf("acceptance: %d\n%s\n%s", code, out, errout)
	}
	assertProtocolCases(t, out)
	assertTemporaryClean(t, root)
}

func TestNegativeProtocolControls(t *testing.T) {
	for _, fault := range []struct{ name, mismatch string }{
		{"all-allow", "expected exit 2"},
		{"all-deny", "expected exit 0"},
		{"empty-reason", "empty denial reason"},
		{"wrong-reason", "wrong denial reason"},
		{"status", "expected exit 2"},
		{"denial-stdout", "denial emitted advice"},
		{"missing-advice", "invalid Claude advice JSON"},
		{"changed-advice", "missing Claude advice"},
		{"non-claude-advice", "unexpected advice on allow"},
	} {
		t.Run(fault.name, func(t *testing.T) {
			root := t.TempDir()
			t.Setenv("TMPDIR", root)
			code, out, _ := runVerification(t, testPackage(t, root, fault.name))
			if code != 1 || !strings.Contains(out, fault.mismatch) || !strings.Contains(out, "FAIL\t") {
				t.Fatalf("control survived: %d\n%s", code, out)
			}
			assertTemporaryClean(t, root)
		})
	}
}

func TestInstalledIdentity(t *testing.T) {
	root := t.TempDir()
	entry := testPackage(t, root, "none")
	alias := filepath.Join(root, "alias")
	if err := os.Symlink(entry, alias); err != nil {
		t.Fatal(err)
	}
	canonical, err := filepath.EvalSymlinks(entry)
	if err != nil {
		t.Fatal(err)
	}
	pkg, err := installed([]string{alias})
	if err != nil || pkg.entry != canonical {
		t.Fatalf("alias identity: %+v, %v", pkg, err)
	}
	for _, binding := range []struct {
		path string
		hash [32]byte
	}{{pkg.entry, pkg.entryHash}, {pkg.native, pkg.nativeHash}} {
		body, err := os.ReadFile(binding.path)
		if err != nil || binding.hash != sha256.Sum256(body) {
			t.Fatalf("wrong executable hash: %s: %v", binding.path, err)
		}
	}
	for _, path := range []string{"relative", filepath.Join(root, "missing"), filepath.Join(root, "package/bin/undeclared")} {
		if filepath.Base(path) == "undeclared" {
			body, err := os.ReadFile(entry)
			if err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(path, body, 0700); err != nil {
				t.Fatal(err)
			}
		}
		if _, err := installed([]string{path}); err == nil {
			t.Fatalf("invalid executable accepted: %s", path)
		}
	}
}

func TestIncompleteInstallation(t *testing.T) {
	for _, fault := range []string{"missing-native", "missing-version", "invalid-version", "not-executable"} {
		t.Run(fault, func(t *testing.T) {
			root := t.TempDir()
			entry := testPackage(t, root, "none")
			var err error
			switch fault {
			case "missing-native":
				err = os.Remove(filepath.Join(filepath.Dir(entry), "agent-guard-native"))
			case "missing-version":
				err = os.Remove(filepath.Join(root, "package/VERSION"))
			case "invalid-version":
				err = os.WriteFile(filepath.Join(root, "package/VERSION"), []byte("development\n"), 0600)
			case "not-executable":
				err = os.Chmod(entry, 0600)
			}
			if err != nil {
				t.Fatal(err)
			}
			if _, err := installed([]string{entry}); err == nil {
				t.Fatalf("incomplete installation accepted: %s", fault)
			}
		})
	}
}

func TestVersionBinding(t *testing.T) {
	root := t.TempDir()
	t.Setenv("TMPDIR", root)
	entry := testPackage(t, root, "none")
	if err := os.WriteFile(filepath.Join(filepath.Dir(entry), "agent-guard-native"), []byte("#!/bin/sh\nprintf 'agent-guard 0.0.1\\n'\n"), 0700); err != nil {
		t.Fatal(err)
	}
	code, out, errout := runVerification(t, entry)
	if code != 1 || !strings.Contains(errout, "does not match VERSION") || strings.Contains(out, "RESULT\t") {
		t.Fatalf("version mismatch accepted: %d, %s, %s", code, out, errout)
	}
	assertTemporaryClean(t, root)
}

func TestRegisteredEvents(t *testing.T) {
	root := t.TempDir()
	t.Setenv("TMPDIR", root)
	code, out, errout := runVerification(t, testPackage(t, root, "event-shape"))
	if code != 0 || errout != "" {
		t.Fatalf("registered event shape: %d, %s, %s", code, out, errout)
	}
	assertProtocolCases(t, out)
}

func TestDeadlineCleanup(t *testing.T) {
	root := t.TempDir()
	t.Setenv("TMPDIR", root)
	entry := testPackage(t, root, "hang")
	started := time.Now()
	code, out, _ := runVerification(t, entry)
	if code != 1 || !strings.Contains(out, "deadline exceeded") || !strings.Contains(out, fmt.Sprintf("1/%d completed", len(protocolCases()))) || time.Since(started) > 6*time.Second {
		t.Fatalf("deadline contract: %d, %s", code, out)
	}
	assertFixtureDead(t, root)
	assertTemporaryClean(t, root)
}

func TestInterruptCleanup(t *testing.T) {
	for _, signal := range []syscall.Signal{syscall.SIGTERM, syscall.SIGINT, syscall.SIGHUP} {
		t.Run(signal.String(), func(t *testing.T) {
			root := t.TempDir()
			entry := testPackage(t, root, "hang")
			ctx, cancel := context.WithTimeout(context.Background(), 8*time.Second)
			defer cancel()
			cmd := exec.CommandContext(ctx, os.Args[0], "-test.run=^TestUtilityProcess$", "--", "verifier", entry)
			cmd.Env = []string{"HOME=" + root, "TMPDIR=" + root, "PATH=/usr/bin:/bin", "GORACE=atexit_sleep_ms=0"}
			cmd.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
			cmd.Cancel = func() error { return cmd.Process.Signal(syscall.SIGTERM) }
			cmd.WaitDelay = time.Second
			var out bytes.Buffer
			cmd.Stdout, cmd.Stderr = &out, &out
			if err := cmd.Start(); err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { _ = syscall.Kill(-cmd.Process.Pid, syscall.SIGKILL) })
			ready := filepath.Join(root, "fixture-pids")
			until := time.Now().Add(2 * time.Second)
			for {
				if _, err := os.Stat(ready); err == nil {
					break
				}
				if time.Now().After(until) {
					_ = cmd.Process.Kill()
					_ = cmd.Wait()
					t.Fatal("fixture did not start")
				}
				time.Sleep(10 * time.Millisecond)
			}
			pid := cmd.Process.Pid
			if signal == syscall.SIGINT {
				pid = -pid
			}
			if err := syscall.Kill(pid, signal); err != nil {
				t.Fatal(err)
			}
			err := cmd.Wait()
			var exit *exec.ExitError
			if !errors.As(err, &exit) || exit.ExitCode() != 128+int(signal) {
				t.Fatalf("interrupt exit: %v, %s", err, out.String())
			}
			assertFixtureDead(t, root)
			assertTemporaryClean(t, root)
		})
	}
}

func assertTemporaryClean(t *testing.T, root string) {
	t.Helper()
	paths, err := filepath.Glob(filepath.Join(root, "agent-guard-acceptance-*"))
	if err != nil || len(paths) != 0 {
		t.Fatalf("temporary HOME survived: %v, %v", paths, err)
	}
}

func assertFixtureDead(t *testing.T, root string) {
	t.Helper()
	data, err := os.ReadFile(filepath.Join(root, "fixture-pids"))
	if err != nil {
		t.Fatal(err)
	}
	pids := strings.Fields(string(data))
	if len(pids) == 0 {
		t.Fatal("missing fixture child PID partition")
	}
	for _, value := range pids {
		pid, err := strconv.Atoi(value)
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = syscall.Kill(pid, syscall.SIGKILL) })
		until := time.Now().Add(time.Second)
		for {
			if err := syscall.Kill(pid, 0); errors.Is(err, syscall.ESRCH) {
				break
			}
			if time.Now().After(until) {
				t.Fatalf("fixture process %d survived", pid)
			}
			time.Sleep(10 * time.Millisecond)
		}
	}
}

func TestUtilityProcess(t *testing.T) {
	index := -1
	for i, arg := range os.Args {
		if arg == "--" {
			index = i
			break
		}
	}
	if index < 0 {
		return
	}
	args := os.Args[index+1:]
	if args[0] == "verifier" {
		os.Exit(runMain(args[1:]))
	}
	fault := args[1]
	if fault == "hang" {
		child := exec.Command("/bin/sleep", "20")
		if err := child.Start(); err != nil {
			os.Exit(7)
		}
		root := filepath.Dir(os.Getenv("TMPDIR"))
		if err := os.WriteFile(filepath.Join(root, "fixture-pids"), []byte(fmt.Sprintf("%d %d", os.Getpid(), child.Process.Pid)), 0600); err != nil {
			os.Exit(7)
		}
		_ = child.Wait()
		os.Exit(0)
	}
	runtime := args[3]
	var event struct {
		ToolName  string            `json:"tool_name"`
		ToolInput map[string]string `json:"tool_input"`
		Cwd       string            `json:"cwd"`
	}
	if err := json.NewDecoder(os.Stdin).Decode(&event); err != nil {
		os.Exit(7)
	}
	if fault == "event-shape" && (event.Cwd == "" || event.ToolInput["cwd"] != "" || runtime == "codex" && event.ToolName != "Bash" || runtime == "pi" && event.ToolName != strings.ToLower(event.ToolName)) {
		os.Exit(7)
	}
	text := event.ToolInput["command"] + event.ToolInput["file_path"] + event.ToolInput["path"]
	reason := ""
	switch {
	case strings.HasPrefix(text, "rg") && strings.Contains(text, "~/Library"):
		reason = "A scan rooted at the home directory or ~/Library"
	case strings.Contains(text, "Library/Containers") || strings.Contains(text, "data-link"):
		reason = "This reads a protected macOS app-data directory."
	case strings.ToLower(event.ToolName) != "write" && (strings.HasSuffix(text, ".env") || strings.HasSuffix(text, "id_rsa") && !strings.HasPrefix(text, "ssh")):
		reason = "This reads a credential or environment file."
	}
	if fault == "all-allow" {
		reason = ""
	} else if fault == "all-deny" {
		reason = "Synthetic unconditional denial"
	}
	if reason != "" {
		if fault == "wrong-reason" {
			reason = "A different denial"
		}
		if fault != "empty-reason" {
			if runtime == "claude" {
				reason = "DENIED: " + reason
			}
			fmt.Fprintln(os.Stderr, reason)
		}
		if fault == "denial-stdout" {
			fmt.Println("Unexpected denial advice")
		}
		if fault == "status" {
			os.Exit(7)
		}
		os.Exit(2)
	}
	if strings.HasPrefix(text, "rg -rn") && fault != "missing-advice" && (runtime == "claude" || fault == "non-claude-advice") {
		advice := "rg -r means --replace. Drop -r; use -n for line numbers, or spell --replace VALUE for an intentional replacement."
		if fault == "changed-advice" {
			advice += " Unexpected extra advice."
		}
		_ = json.NewEncoder(os.Stdout).Encode(map[string]any{"hookSpecificOutput": map[string]string{"hookEventName": "PreToolUse", "additionalContext": advice}})
	}
	os.Exit(0)
}
