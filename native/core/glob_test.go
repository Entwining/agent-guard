package core

import (
	"testing"

	"agentguard/native/filesystem"
	"agentguard/native/reasons"
)

func TestGlobPanicRegression(t *testing.T) {
	home := syntheticHome(t)
	for _, command := range []string{"echo [^[:alpha:]]", "cat [a[:digit:]]", "cat [^[:alpha:]]", "cat ./[^[:alpha:]]x", "ls ~/Library/[^[:alpha:]]x", "ls [^[:alpha:]]", "cat [^[:alpha:]", "cat ~/.aws/[a[:digit:]]"} {
		req := BuildRequest("codex", "bash", home+"/project", command, "", home)
		if got := verdict(t, req, filesystem.DiskProbe{}); got != "" {
			t.Errorf("%s: %q", command, got)
		}
	}
	for _, command := range []string{"cat ~/.aws/[[:alpha:]]*", "cat .[[:alpha:]]nv"} {
		req := BuildRequest("codex", "bash", home+"/project", command, "", home)
		if got := verdict(t, req, filesystem.DiskProbe{}); got != reasons.File {
			t.Errorf("%s: %q", command, got)
		}
	}
	req := BuildRequest("claude", "grep", home+"/project", home+"/project", "{.env}*", home)
	if got := verdict(t, req, filesystem.DiskProbe{}); got != reasons.File {
		t.Fatalf("API glob: %q", got)
	}
}
