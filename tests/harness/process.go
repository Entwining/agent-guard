package harness

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
	"syscall"
	"time"
)

type ProcessResult struct {
	Status       int     `json:"status"`
	Stdout       string  `json:"stdout"`
	Stderr       string  `json:"stderr"`
	PID          int     `json:"pid"`
	TimedOut     bool    `json:"timedOut"`
	SpawnError   string  `json:"spawnError,omitempty"`
	WaitError    string  `json:"waitError,omitempty"`
	Milliseconds float64 `json:"ms"`
}

// A separate process group confines timeout cleanup to this synthetic invocation.
// The observer runs before harness cleanup so cleanup cannot manufacture a pass.
func runProcess(argv []string, input []byte, cwd string, env []string, deadline time.Duration, observe func(ProcessResult) error) (result ProcessResult, failure error) {
	started := time.Now()
	ctx, cancel := context.WithTimeout(context.Background(), deadline)
	defer cancel()
	cmd := exec.CommandContext(ctx, argv[0], argv[1:]...)
	cmd.Dir, cmd.Env = cwd, env
	cmd.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
	cmd.Stdin = bytes.NewReader(input)
	var stdout, stderr bytes.Buffer
	cmd.Stdout, cmd.Stderr = &stdout, &stderr
	cmd.WaitDelay = time.Second
	cmd.Cancel = func() error { return killGroup(cmd.Process.Pid) }
	result = ProcessResult{Status: -1}
	if err := cmd.Start(); err != nil {
		result.SpawnError = err.Error()
		result.Milliseconds = float64(time.Since(started).Microseconds()) / 1000
		return result, nil
	}
	result.PID = cmd.Process.Pid
	defer func() { failure = errors.Join(failure, killGroup(result.PID)) }()
	err := cmd.Wait()
	result.Status = cmd.ProcessState.ExitCode()
	result.Stdout, result.Stderr = stdout.String(), stderr.String()
	result.TimedOut = ctx.Err() != nil
	result.Milliseconds = float64(time.Since(started).Microseconds()) / 1000
	var exit *exec.ExitError
	if err != nil && !errors.As(err, &exit) {
		result.WaitError = err.Error()
	}
	if observe != nil {
		return result, observe(result)
	}
	return result, nil
}

func killGroup(pid int) error {
	if pid <= 0 {
		return errors.New("invalid process group")
	}
	err := syscall.Kill(-pid, syscall.SIGKILL)
	if errors.Is(err, syscall.ESRCH) {
		return nil
	}
	return err
}

func alive(pid int) (bool, error) {
	err := syscall.Kill(pid, 0)
	if errors.Is(err, syscall.ESRCH) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	return true, nil
}

type Binding struct {
	Path   string      `json:"path"`
	SHA256 string      `json:"sha256"`
	Mode   fs.FileMode `json:"mode"`
}

func hashFile(path, name string) (Binding, error) {
	body, err := os.ReadFile(path)
	if err != nil {
		return Binding{}, err
	}
	info, err := os.Stat(path)
	if err != nil {
		return Binding{}, err
	}
	sum := sha256.Sum256(body)
	return Binding{name, hex.EncodeToString(sum[:]), info.Mode().Perm()}, nil
}

func copyFile(from, to string) error {
	body, err := os.ReadFile(from)
	if err != nil {
		return err
	}
	info, err := os.Stat(from)
	if err != nil {
		return err
	}
	if err = os.MkdirAll(filepath.Dir(to), 0700); err != nil {
		return err
	}
	return os.WriteFile(to, body, info.Mode().Perm())
}

func copyTree(from, to string) error {
	return filepath.WalkDir(from, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		rel, err := filepath.Rel(from, path)
		if err != nil {
			return err
		}
		if d.IsDir() {
			return os.MkdirAll(filepath.Join(to, rel), 0700)
		}
		if !d.Type().IsRegular() {
			return fmt.Errorf("unexpected source type: %s", path)
		}
		return copyFile(path, filepath.Join(to, rel))
	})
}

func SourceBindings(root string) ([]Binding, error) {
	var bindings []Binding
	for _, name := range []string{"go.mod", "go.sum", "bin/agent-guard", "native", "cmd", "tests/harness"} {
		err := filepath.WalkDir(filepath.Join(root, name), func(path string, d fs.DirEntry, err error) error {
			if err != nil {
				return err
			}
			if d.IsDir() {
				return nil
			}
			if !d.Type().IsRegular() {
				return fmt.Errorf("unexpected source type: %s", path)
			}
			rel, err := filepath.Rel(root, path)
			if err != nil {
				return err
			}
			binding, err := hashFile(path, rel)
			if err == nil {
				bindings = append(bindings, binding)
			}
			return err
		})
		if err != nil {
			return nil, err
		}
	}
	sort.Slice(bindings, func(i, j int) bool { return bindings[i].Path < bindings[j].Path })
	return bindings, nil
}

func writeJSON(path string, value any) error {
	body, err := json.MarshalIndent(value, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(path, append(body, '\n'), 0600)
}

func quote(value string) string { return "'" + strings.ReplaceAll(value, "'", "'\\''") + "'" }

// Resolve one component at a time, rejecting protected aliases before probing them.
func outsidePath(path string) (string, error) {
	abs, err := filepath.Abs(path)
	if err != nil {
		return "", err
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	protected := func(value string) bool {
		for _, name := range []string{"Library", ".ssh", ".gnupg", ".aws", ".azure", ".config/gcloud"} {
			root := filepath.Join(home, name)
			if value == root || strings.HasPrefix(value, root+"/") {
				return true
			}
		}
		return false
	}
	resolved := "/"
	parts := strings.Split(strings.TrimPrefix(abs, "/"), "/")
	links := 0
	for len(parts) > 0 {
		resolved = filepath.Join(resolved, parts[0])
		parts = parts[1:]
		if protected(resolved) {
			return "", errors.New("output must stay outside protected paths")
		}
		info, err := os.Lstat(resolved)
		if err != nil && !errors.Is(err, os.ErrNotExist) {
			return "", err
		}
		if err == nil && info.Mode()&os.ModeSymlink != 0 {
			links++
			if links > 40 {
				return "", errors.New("too many output symlinks")
			}
			target, err := os.Readlink(resolved)
			if err != nil {
				return "", err
			}
			if !filepath.IsAbs(target) {
				target = filepath.Join(filepath.Dir(resolved), target)
			}
			parts = append(strings.Split(strings.TrimPrefix(filepath.Clean(target), "/"), "/"), parts...)
			resolved = "/"
			continue
		}
		if _, err := os.Lstat(filepath.Join(resolved, ".git")); err == nil {
			return "", errors.New("output must stay outside Git checkouts")
		} else if !errors.Is(err, os.ErrNotExist) && !errors.Is(err, syscall.ENOTDIR) {
			return "", err
		}
	}
	return resolved, nil
}

func newOutput(path string) (string, error) {
	if path == "" {
		return "", errors.New("a new output directory is required")
	}
	output, err := outsidePath(path)
	if err != nil {
		return "", err
	}
	if err = os.MkdirAll(filepath.Dir(output), 0700); err != nil {
		return "", err
	}
	if err = os.Mkdir(output, 0700); err != nil {
		return "", err
	}
	return output, nil
}
