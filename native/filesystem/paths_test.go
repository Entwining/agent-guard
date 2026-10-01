package filesystem

import (
	"os"
	"path/filepath"
	"testing"
)

func TestRelativeResolve(t *testing.T) {
	cwd, e := os.Getwd()
	if e != nil {
		t.Fatal(e)
	}
	want := filepath.Join(cwd, "project", "file.txt")
	if got := Resolve("project", "file.txt"); got != want {
		t.Fatalf("relative base resolved to %q, want %q", got, want)
	}
	if got := Resolve("project", "/absolute/file.txt"); got != "/absolute/file.txt" {
		t.Fatal(got)
	}
}

func TestUsernameHomeExpansion(t *testing.T) {
	t.Setenv("USER", "synthetic-operator")
	if ExpandHome("~synthetic-operator/Library/Containers", "/synthetic-home") != "/synthetic-home/Library/Containers" {
		t.Fatal("named current-user tilde was not expanded")
	}
	if ExpandHome("~other/Library/Containers", "/synthetic-home") != "~other/Library/Containers" {
		t.Fatal("another user was expanded")
	}
	if e := os.Unsetenv("USER"); e != nil {
		t.Fatal(e)
	}
	if ExpandHome("~unknown/project", "/synthetic-home") != "/synthetic-home/project" {
		t.Fatal("minimal environment username differs from frozen Bun")
	}
	t.Setenv("USER", "")
	if ExpandHome("~unknown/project", "/synthetic-home") != "~unknown/project" {
		t.Fatal("empty USER differs from missing USER")
	}
}
