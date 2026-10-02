package core

import (
	"context"

	"agentguard/native/filesystem"
	"agentguard/native/reasons"
	"agentguard/native/record"
	"agentguard/native/rules"
	"agentguard/native/shell"
	"agentguard/native/targets"
)

func BuildRequest(runtime, tool, cwd, input, glob, home string) record.Request {
	if cwd == "" {
		cwd = "/"
	}
	r := record.Request{Runtime: runtime, Tool: tool, Home: home, Cwd: filesystem.AbsPath(cwd, "/", home), InputCwd: cwd, PathInput: input, Glob: glob, Script: record.Script{Commands: []*record.Command{}, Uninspectable: []record.Fragment{}}}
	if tool == "bash" {
		if input != "" {
			r.Script = shell.ParseScript(input, cwd, home)
		}
	} else if tool == "grep" {
		r.Operation = "search"
		p := input
		if p == "" {
			p = r.Cwd
		}
		r.SearchRoot = filesystem.AbsPath(p, r.Cwd, home)
	} else if input != "" {
		r.Operation = "write"
		if tool == "read" {
			r.Operation = "read"
		}
	}
	return r
}

func Evaluate(ctx context.Context, req record.Request, probe filesystem.Probe) (string, error) {
	if e := ctx.Err(); e != nil {
		return "", e
	}
	if e := filesystem.InitializationError(); e != nil {
		return "", e
	}
	if req.ParseFailed {
		return reasons.Syntax, nil
	}
	ts := targets.ExtractTargets(req)
	d := append(rules.Appdata(req, ts), rules.Credentials(req, ts)...)
	if len(d) > 0 {
		return d[0], nil
	}
	linked, e := filesystem.LinkedTargets(ctx, ts, req.Home, probe)
	if e != nil {
		if canceled := ctx.Err(); canceled != nil {
			return "", canceled
		}
		return reasons.Symlink, nil
	}
	if linked != nil {
		d = append(rules.Appdata(req, linked), rules.Credentials(req, linked)...)
		ts = linked
	}
	if len(d) == 0 {
		d, e = rules.CredentialFilesystem(ctx, req, ts, probe)
		if e != nil {
			if canceled := ctx.Err(); canceled != nil {
				return "", canceled
			}
			return reasons.Symlink, nil
		}
	}
	if e := ctx.Err(); e != nil {
		return "", e
	}
	if len(d) > 0 {
		return d[0], nil
	}
	return "", nil
}

func Suggestions(req record.Request) []string {
	if req.Runtime == "claude" {
		return rules.Workflow(req)
	}
	return []string{}
}
