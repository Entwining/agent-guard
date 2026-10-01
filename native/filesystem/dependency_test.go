package filesystem_test

import (
	"agentguard/native/core"
	"agentguard/native/filesystem"
	"errors"
	"syscall"
	"testing"
)

func TestInitializationFailure(t *testing.T) {
	filesystem.InjectInitializationFailure(t, syscall.EIO)
	for _, command := range []string{"echo ok", "env"} {
		req := core.BuildRequest("codex", "bash", "/synthetic-home/project", command, "", "/synthetic-home")
		if reason, e := core.Evaluate(t.Context(), req, filesystem.DiskProbe{}); reason != "" || !errors.Is(e, syscall.EIO) {
			t.Fatalf("initialization failure became a verdict: %q %v", reason, e)
		}
	}
	if _, e := core.CheckEvent(t.Context(), "codex", "/synthetic-home/project", "/synthetic-home", []byte(`{"tool_name":"WebFetch","tool_input":{}}`)); !errors.Is(e, syscall.EIO) {
		t.Fatalf("unknown tool hid the initialization failure: %v", e)
	}
}
