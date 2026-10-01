package core

import (
	"agentguard/native/filesystem"
	"agentguard/native/reasons"
	"testing"
)

func TestAssignmentProtectedSubstitution(t *testing.T) {
	home := syntheticHome(t)
	for _, source := range []string{"x=$(cat .env)", "export x=$(cat .env)", "x=`cat .env`", "f(){ cat .env; }; x=$(f)", "x=$(y=$(cat .env))"} {
		req := BuildRequest("claude", "bash", home+"/project", source, "", home)
		if got := verdict(t, req, filesystem.DiskProbe{}); got != reasons.File {
			t.Fatalf("%s: %q", source, got)
		}
	}
}
