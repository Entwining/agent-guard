package main

import (
	"bytes"
	"context"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"

	"agentguard/native/reasons"
)

func TestNativeEntry(t *testing.T) {
	_, file, _, _ := runtime.Caller(0)
	source := filepath.Clean(filepath.Join(filepath.Dir(file), "../.."))
	entrySource, e := os.ReadFile(source + "/bin/agent-guard")
	if e != nil {
		t.Fatal(e)
	}
	build := t.TempDir()
	binary := build + "/agent-guard-native"
	goBinary := os.Getenv("GO")
	if goBinary == "" {
		goBinary, e = exec.LookPath("go")
		if e != nil {
			t.Fatal(e)
		}
	}
	cacheCommand := exec.Command(goBinary, "env", "-json", "GOMODCACHE", "GOCACHE")
	cacheCommand.Dir = source
	cacheOutput, e := cacheCommand.Output()
	if e != nil {
		t.Fatalf("resolve Go caches: %v", e)
	}
	var caches struct {
		GOMODCACHE string
		GOCACHE    string
	}
	if e := json.Unmarshal(cacheOutput, &caches); e != nil {
		t.Fatal(e)
	}
	compiler := exec.Command(goBinary, "build", "-race", "-trimpath", "-ldflags", "-X main.version=0.0.0", "-o", binary, "./cmd/agent-guard")
	compiler.Dir = source
	compiler.Env = []string{"PATH=/usr/bin:/bin", "HOME=" + build, "TMPDIR=" + os.Getenv("TMPDIR"), "GOMODCACHE=" + caches.GOMODCACHE, "GOCACHE=" + caches.GOCACHE, "GOPROXY=off"}
	if b, e := compiler.CombinedOutput(); e != nil {
		t.Fatalf("build: %v: %s", e, b)
	}
	executable, e := os.ReadFile(binary)
	if e != nil {
		t.Fatal(e)
	}
	install := func(t *testing.T) (string, string) {
		t.Helper()
		home, e := filepath.EvalSymlinks(t.TempDir())
		if e != nil {
			t.Fatal(e)
		}
		for _, dir := range []string{"package/bin", "project", "Library/Containers/com.x"} {
			if e := os.MkdirAll(home+"/"+dir, 0755); e != nil {
				t.Fatal(e)
			}
		}
		entry := home + "/package/bin/agent-guard"
		if e := os.WriteFile(entry, entrySource, 0755); e != nil {
			t.Fatal(e)
		}
		if e := os.WriteFile(home+"/package/bin/agent-guard-native", executable, 0755); e != nil {
			t.Fatal(e)
		}
		return home, entry
	}
	t.Run("version", func(t *testing.T) {
		home, entry := install(t)
		ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
		defer cancel()
		command := exec.CommandContext(ctx, entry, "--version")
		command.Env = []string{"HOME=" + home, "PATH=/usr/bin:/bin", "GORACE=atexit_sleep_ms=0"}
		output, err := command.CombinedOutput()
		if err != nil || string(output) != "agent-guard 0.0.0\n" {
			t.Fatalf("version: %v: %q", err, output)
		}
	})
	t.Run("checker-contract", func(t *testing.T) {
		home, entry := install(t)
		assertCheckerContracts(t, home, entry)
	})

	run := func(t *testing.T, home, entry, cwd, body string, extraEnv, extraArgs []string) (int, string, string) {
		t.Helper()
		ctx, cancel := context.WithTimeout(context.Background(), 8*time.Second)
		defer cancel()
		args := append([]string{"--runtime", "codex"}, extraArgs...)
		command := exec.CommandContext(ctx, entry, args...)
		command.Dir = cwd
		command.Env = append([]string{"HOME=" + home, "PATH=/usr/bin:/bin"}, extraEnv...)
		command.Stdin = strings.NewReader(body)
		var out, errout bytes.Buffer
		command.Stdout = &out
		command.Stderr = &errout
		e := command.Run()
		if ctx.Err() != nil {
			t.Fatal("entry exceeded its deadline")
		}
		exit := 0
		if e != nil {
			if err, ok := e.(*exec.ExitError); ok {
				exit = err.ExitCode()
			} else {
				t.Fatal(e)
			}
		}
		return exit, out.String(), errout.String()
	}
	for _, runtime := range []string{"claude", "codex", "pi"} {
		t.Run("runtime-"+runtime, func(t *testing.T) {
			home, entry := install(t)
			exit, out, errout := run(t, home, entry, home+"/project", `{"tool_input":{"command":"env"}}`, nil, []string{"--runtime", runtime})
			want := reasons.Dump
			if runtime == "claude" {
				want = "DENIED: " + want + " Do NOT bypass this restriction or retry the same blocked command."
			}
			if exit != 2 || out != "" || errout != want+"\n" {
				t.Fatalf("%d %q %q", exit, out, errout)
			}
		})
	}
	t.Run("multi-target-resolution", func(t *testing.T) {
		home, entry := install(t)
		body, e := json.Marshal(map[string]any{"tool_input": map[string]any{"command": "printf '%s\\n' a b | xargs cat"}})
		if e != nil {
			t.Fatal(e)
		}
		exit, out, errout := run(t, home, entry, home+"/project", string(body), []string{"GORACE=atexit_sleep_ms=0"}, nil)
		if exit != 0 || out != "" || errout != "" {
			t.Fatalf("%d %q %q", exit, out, errout)
		}
	})
	t.Run("hook-cwd-and-linked-entry", func(t *testing.T) {
		home, entry := install(t)
		alias := home + "/linked-entry"
		if e := os.Symlink(entry, alias); e != nil {
			t.Fatal(e)
		}
		for _, c := range []struct {
			cwd  string
			exit int
		}{{home, 2}, {home + "/project", 0}} {
			exit, _, _ := run(t, home, alias, c.cwd, `{"tool_input":{"command":"ls Library/Containers"}}`, nil, nil)
			if exit != c.exit {
				t.Fatalf("cwd %s exit %d", c.cwd, exit)
			}
		}
	})
	t.Run("event-cwd-wins", func(t *testing.T) {
		home, entry := install(t)
		body, _ := json.Marshal(map[string]any{"cwd": home, "tool_input": map[string]any{"cwd": home + "/project", "command": "ls Library/Containers"}})
		exit, _, errout := run(t, home, entry, home+"/project", string(body), nil, nil)
		if exit != 2 || errout != reasons.Appdata+"\n" {
			t.Fatalf("%d %q", exit, errout)
		}
	})
	t.Run("bash-startup-file", func(t *testing.T) {
		home, entry := install(t)
		marker := home + "/startup-loaded"
		if e := os.WriteFile(home+"/startup.sh", []byte("printf loaded > '"+marker+"'\nexit 0\n"), 0600); e != nil {
			t.Fatal(e)
		}
		exit, out, errout := run(t, home, entry, home+"/project", `{"tool_input":{"command":"env"}}`, []string{"BASH_ENV=" + home + "/startup.sh"}, nil)
		if exit != 2 || out != "" || errout != reasons.Dump+"\n" {
			t.Fatalf("%d %q %q", exit, out, errout)
		}
		if _, e := os.Stat(marker); !os.IsNotExist(e) {
			t.Fatal("BASH_ENV executed")
		}
	})
	t.Run("relative-event-cwd", func(t *testing.T) {
		home, entry := install(t)
		exit, out, errout := run(t, home, entry, home, `{"tool_input":{"cwd":"..","command":"ls Library/Containers"}}`, nil, nil)
		if exit != 2 || out != "" || errout != reasons.Appdata+"\n" {
			t.Fatalf("relative event cwd must resolve from package: %d %q %q", exit, out, errout)
		}
	})
	t.Run("home-spelling", func(t *testing.T) {
		home, entry := install(t)
		if e := os.Symlink(home, home+"-alias"); e != nil {
			t.Fatal(e)
		}
		t.Cleanup(func() { _ = os.Remove(home + "-alias") })
		if e := os.Symlink(home+"/Library/Containers", home+"/project/data-link"); e != nil {
			t.Fatal(e)
		}
		for _, spelling := range []string{home + "/", home + "-alias"} {
			exit, _, errout := run(t, spelling, entry, spelling+"/project", `{"tool_input":{"command":"cat data-link/com.x/file.txt"}}`, nil, nil)
			if exit != 2 || errout != reasons.Appdata+"\n" {
				t.Fatalf("%s: %d %q", spelling, exit, errout)
			}
		}
		exit, _, errout := run(t, "home", entry, home+"/project", `{"tool_input":{"command":"ls"}}`, nil, nil)
		if exit != 2 || !strings.Contains(errout, "HOME is not an absolute path") {
			t.Fatalf("%d %q", exit, errout)
		}
	})
	for _, fault := range []string{"missing-binary", "broken-binary"} {
		t.Run(fault, func(t *testing.T) {
			home, entry := install(t)
			binary := home + "/package/bin/agent-guard-native"
			if fault == "missing-binary" {
				e = os.Remove(binary)
			} else {
				e = os.WriteFile(binary, []byte("#!/bin/sh\nexit 7\n"), 0755)
			}
			if e != nil {
				t.Fatal(e)
			}
			exit, out, errout := run(t, home, entry, home+"/project", `{"tool_input":{"command":"ls"}}`, nil, nil)
			if exit != 2 || out != "" || !strings.Contains(errout, "so this call is blocked") {
				t.Fatalf("%d %q %q", exit, out, errout)
			}
		})
	}
	t.Run("malformed", func(t *testing.T) {
		home, entry := install(t)
		for _, body := range []string{`null`, `[]`, `{`, `{"tool_input":"cat .env"}`, `{"tool_input":{"command":["cat",".env"]}}`, `{"tool_input":{}}`, `{"tool_name":"Read","tool_input":{"path":".env"}}`, `{"tool_name":"Grep","tool_input":{"path":5}}`} {
			exit, out, errout := run(t, home, entry, home+"/project", body, nil, nil)
			if exit != 2 || out != "" || !strings.Contains(errout, "so this call is blocked") {
				t.Fatalf("%s: %d %q %q", body, exit, out, errout)
			}
		}
	})
	t.Run("ambiguous-cli", func(t *testing.T) {
		home, entry := install(t)
		exit, out, errout := run(t, home, entry, home+"/project", `{"tool_input":{"command":"ls"}}`, nil, []string{"--cwd", "-foo"})
		if exit != 2 || out != "" || errout != usage+"\n" {
			t.Fatalf("%d %q %q", exit, out, errout)
		}
	})
	t.Run("dash-cwd-value", func(t *testing.T) {
		home, entry := install(t)
		exit, out, errout := run(t, home, entry, home+"/project", `{"tool_input":{"command":"ls"}}`, nil, []string{"--cwd", "-"})
		if exit != 0 || out != "" || errout != "" {
			t.Fatalf("literal dash was treated as an option: %d %q %q", exit, out, errout)
		}
	})
}
