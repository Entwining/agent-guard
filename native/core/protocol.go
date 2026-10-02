package core

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"strings"

	"agentguard/native/filesystem"
)

type Result struct {
	Exit           int
	Stdout, Stderr []byte
}

func CheckEvent(ctx context.Context, runtime, hookCwd, home string, body []byte) (Result, error) {
	if e := ctx.Err(); e != nil {
		return Result{}, e
	}
	if e := filesystem.InitializationError(); e != nil {
		return Result{}, e
	}
	var event map[string]any
	if e := json.Unmarshal(bytes.TrimPrefix(body, []byte{0xef, 0xbb, 0xbf}), &event); e != nil {
		return Result{}, e
	}
	if event == nil {
		return Result{}, fmt.Errorf("event is not an object")
	}
	input, _ := event["tool_input"].(map[string]any)
	str := func(x any) string { v, _ := x.(string); return v }
	cwd := hookCwd
	if v, ok := input["cwd"].(string); ok {
		cwd = v
	}
	if v, ok := event["cwd"].(string); ok {
		cwd = v
	} else if event["tool_input"] == nil {
		return Result{}, fmt.Errorf("tool_input is missing or null")
	}
	name := "Bash"
	if n, ok := event["tool_name"].(string); ok {
		name = n
	}
	tool, field := "", ""
	switch strings.ToLower(name) {
	case "bash":
		tool, field = "bash", "command"
	case "read", "write", "edit":
		tool, field = strings.ToLower(name), "file_path"
	case "grep":
		tool, field = "grep", "path"
	default:
		return Result{}, nil
	}
	if event["tool_input"] == nil {
		return Result{}, fmt.Errorf("tool_input is missing or null")
	}
	value, ok := input[field].(string)
	if tool == "grep" {
		if _, present := input[field]; !present {
			value = ""
			ok = true
		}
	}
	if !ok {
		return Result{}, fmt.Errorf("tool_input.%s is not a string", field)
	}
	req := BuildRequest(runtime, tool, cwd, value, str(input["glob"]), home)
	reason, e := Evaluate(ctx, req, filesystem.DiskProbe{})
	if e != nil {
		return Result{}, e
	}
	if reason != "" {
		if runtime == "claude" {
			reason = "DENIED: " + reason + " Do NOT bypass this restriction or retry the same blocked command."
		}
		return Result{Exit: 2, Stderr: []byte(reason + "\n")}, nil
	}
	advice := Suggestions(req)
	if len(advice) == 0 {
		return Result{}, nil
	}
	context, _ := json.Marshal(strings.Join(advice, "\n"))
	body = []byte(`{"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":` + string(context) + "}}\n")
	return Result{Stdout: body}, nil
}
