package main

import (
	"agentguard/native/reasons"
	"bytes"
	"context"
	"encoding/json"
	"os/exec"
	"strings"
	"testing"
	"time"
)

func assertCheckerContracts(t *testing.T, home, entry string) {
	t.Helper()
	for _, runtime := range []string{"claude", "codex", "pi"} {
		for _, row := range []struct {
			name, command string
			extra         []string
			status        int
		}{
			{"allow", "ls", nil, 0},
			{"denial", "env", nil, 2},
			{"advice", "rg -rn foo src", nil, 0},
			{"usage", "ls", []string{"--other"}, 2},
		} {
			body, e := json.Marshal(map[string]any{"tool_input": map[string]any{"command": row.command}})
			if e != nil {
				t.Fatal(e)
			}
			wantOut, wantErr := "", ""
			if row.name == "denial" {
				wantErr = reasons.Dump
				if runtime == "claude" {
					wantErr = "DENIED: " + wantErr + " Do NOT bypass this restriction or retry the same blocked command."
				}
				wantErr += "\n"
			}
			if row.name == "advice" && runtime == "claude" {
				advice, e := json.Marshal(reasons.Replace)
				if e != nil {
					t.Fatal(e)
				}
				wantOut = `{"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":` + string(advice) + "}}\n"
			}
			if row.name == "usage" {
				wantErr = usage + "\n"
			}
			for _, mode := range []string{"entry", "--checker", "--supervised-checker"} {
				if mode == "entry" && row.name == "denial" {
					continue
				}

				t.Run(runtime+"/"+row.name+"/"+mode, func(t *testing.T) {
					path := entry
					args := []string{"--runtime", runtime, "--cwd", home + "/project"}
					wantStatus := row.status
					if mode != "entry" {
						path = home + "/package/bin/agent-guard-native"
						args = append([]string{mode}, args...)
						if mode == "--supervised-checker" && wantStatus == 2 {
							wantStatus = checkerDenial
						}
					}
					args = append(args, row.extra...)
					ctx, cancel := context.WithTimeout(t.Context(), 8*time.Second)
					defer cancel()
					child := exec.CommandContext(ctx, path, args...)
					child.Dir = home + "/project"
					child.Env = []string{"HOME=" + home, "PATH=/usr/bin:/bin", "GORACE=atexit_sleep_ms=0"}
					child.Stdin = strings.NewReader(string(body))
					var stdout, stderr bytes.Buffer
					child.Stdout, child.Stderr = &stdout, &stderr
					e := child.Run()
					if ctx.Err() != nil {
						t.Fatal(ctx.Err())
					}
					status := 0
					if e != nil {
						exit, ok := e.(*exec.ExitError)
						if !ok {
							t.Fatal(e)
						}
						status = exit.ExitCode()
					}
					if status != wantStatus || stdout.String() != wantOut || stderr.String() != wantErr {
						t.Fatalf("status %d stdout %q stderr %q; want %d %q %q", status, stdout.String(), stderr.String(), wantStatus, wantOut, wantErr)
					}
				})
			}
		}
	}
}
