package main

import (
	"agentguard/native/core"
	"context"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"syscall"
	"time"
)

// Only a completed supervised rejection uses 3; Go's default panic status is 2.
const checkerDenial = 3

var version = "development"

const usage = "usage: agent-guard --runtime claude|codex|pi < event.json"

func main() {
	if len(os.Args) > 1 {
		switch os.Args[1] {
		case "--version":
			if len(os.Args) != 2 {
				fmt.Fprintln(os.Stderr, usage)
				os.Exit(2)
			}
			fmt.Println("agent-guard " + version)
			return
		case "--checker":
			os.Exit(check(os.Args[2:]))
		case "--supervised-checker":
			os.Exit(checkerStatus(check(os.Args[2:])))
		}
	}
	os.Exit(run(os.Args[1:]))
}

func run(args []string) int {
	self, e := os.Executable()
	if e != nil {
		fmt.Fprintln(os.Stderr, e)
		return 1
	}
	ctx, cancel := context.WithTimeout(context.Background(), 2800*time.Millisecond)
	defer cancel()
	child := exec.CommandContext(ctx, self, append([]string{"--supervised-checker"}, args...)...)
	child.Stdin = os.Stdin
	child.Stdout = os.Stdout
	child.Stderr = os.Stdout
	e = child.Run()
	if e == nil {
		return 0
	}
	if exit, ok := e.(*exec.ExitError); ok {
		if status, ok := exit.Sys().(syscall.WaitStatus); ok && status.Signaled() {
			return 128 + int(status.Signal())
		}
		return exit.ExitCode()
	}
	return 1
}

func checkerStatus(status int) int {
	if status == 2 {
		return checkerDenial
	}
	return status
}

func check(args []string) int {
	ctx, cancel := context.WithTimeout(context.Background(), 2500*time.Millisecond)
	defer cancel()
	runtime, cwd := "", ""
	for i := 0; i < len(args); i++ {
		s := args[i]
		key, value, eq := strings.Cut(s, "=")
		if key != "--runtime" && key != "--cwd" {
			fmt.Fprintln(os.Stderr, usage)
			return 2
		}
		if !eq {
			i++
			if i >= len(args) {
				fmt.Fprintln(os.Stderr, usage)
				return 2
			}
			value = args[i]
			if value != "-" && strings.HasPrefix(value, "-") {
				fmt.Fprintln(os.Stderr, usage)
				return 2
			}
		}
		if key == "--runtime" {
			runtime = value
		} else {
			cwd = value
		}
	}
	if (runtime != "claude" && runtime != "codex" && runtime != "pi") || cwd == "" {
		fmt.Fprintln(os.Stderr, usage)
		return 2
	}
	body, e := io.ReadAll(os.Stdin)
	if e != nil {
		fmt.Fprintln(os.Stderr, e)
		return 1
	}
	home, e := filepath.EvalSymlinks(os.Getenv("HOME"))
	if e != nil {
		fmt.Fprintln(os.Stderr, e)
		return 1
	}
	home, e = filepath.Abs(home)
	if e != nil {
		fmt.Fprintln(os.Stderr, e)
		return 1
	}
	result, e := core.CheckEvent(ctx, runtime, cwd, home, body)
	if e != nil {
		fmt.Fprintln(os.Stderr, e)
		return 1
	}
	if _, e = os.Stdout.Write(result.Stdout); e != nil {
		return 1
	}
	if _, e = os.Stderr.Write(result.Stderr); e != nil {
		return 1
	}
	return result.Exit
}
