package shell

import (
	"slices"
	"testing"

	"agentguard/native/record"
)

func TestSubshellVariables(t *testing.T) {
	script := ParseScript(`export BASE=/synthetic/parent; (cat "$BASE/inherited"; export BASE=/synthetic/child; cat "$BASE/changed"); cat "$BASE/after"`, "/synthetic/project", "/synthetic")
	if script.ParseFailed {
		t.Fatal("ordinary subshell failed to parse")
	}
	paths := []string{}
	for _, command := range script.Commands {
		if record.Text(command.Argv, 0) == "cat" {
			paths = append(paths, record.Text(command.Argv, 1))
		}
	}
	want := []string{"/synthetic/parent/inherited", "/synthetic/child/changed", "/synthetic/parent/after"}
	if !slices.Equal(paths, want) {
		t.Fatalf("subshell variable inheritance and isolation: %q; want %q", paths, want)
	}
}
