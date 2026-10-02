package rules

import (
	"context"
	"slices"

	"agentguard/native/filesystem"
	"agentguard/native/reasons"
	"agentguard/native/record"
)

func targetReason(t record.Target, home string) string {
	reason := reasons.File
	if t.Via == "code" {
		reason = reasons.CodeFile
	} else if t.Sends {
		reason = reasons.Upload
	}
	if t.Effect == "write" {
		if filesystem.SSHPrivate(t.Path) {
			return reasons.Ssh
		}
		return ""
	}
	if t.Effect != "read" {
		return ""
	}
	if t.Walk == "hidden" {
		return reasons.HiddenSearch
	}
	sensitive := filesystem.IsSensitive(t.Path, t.Glob)
	if t.Via == "tool" && t.Glob {
		sensitive = filesystem.IsSensitiveAPI(t.Path)
	}
	if sensitive || !t.Glob && filesystem.IsSensitiveRoot(t.Path, home) {
		return reason
	}
	return ""
}

func Credentials(req record.Request, ts []record.Target) []string {
	m, _ := groups(ts)
	denials := []string{}
	judge := func(i int) {
		for _, t := range m[i] {
			if r := targetReason(t, req.Home); r != "" {
				denials = append(denials, r)
			}
		}
	}
	judge(-1)
	for i, c := range req.Commands {
		judge(i)
		denials = append(denials, SecretReasons(c)...)
	}
	for _, f := range req.Uninspectable {
		denials = append(denials, SecretSignatures(f.Text)...)
	}
	return denials
}

func CredentialFilesystem(ctx context.Context, req record.Request, ts []record.Target, probe filesystem.Probe) ([]string, error) {
	for _, t := range ts {
		if e := ctx.Err(); e != nil {
			return nil, e
		}
		if !slices.Contains([]string{"read", "write", "list"}, t.Effect) || t.Via == "items" || t.Via == "tool" && t.Glob {
			continue
		}
		if (t.Via == "cwd" || t.Via == "scan") && !t.Search {
			continue
		}
		hit, e := filesystem.SSHScopeDenied(ctx, t.Path, req.Home, t.Search, !t.Expands, probe)
		if e != nil {
			return nil, e
		}
		if hit {
			r := reasons.Ssh
			if t.Search {
				r = reasons.GrepSsh
			}
			return []string{r}, nil
		}
	}
	return nil, nil
}
