package filesystem

import (
	"context"
	"os"
	"os/exec"
	"slices"
	"strings"
	"testing"
	"time"
)

func TestGlobConsumers(t *testing.T) {
	root := t.TempDir()
	subjects := []string{"a", "b", "c", "d", "7", "9", "z", "-", "]", ".env", "{.env}literal", "!literal.env"}
	for _, name := range subjects {
		if err := os.WriteFile(root+"/"+name, nil, 0600); err != nil {
			t.Fatal(err)
		}
	}
	for _, pattern := range []string{"[[:alpha:]]", "[^[:alpha:]]", "[![:alpha:]]", "[a[:digit:]]", "[a-z[:digit:]]", "[]a]", "[-a]", "[a-]", "[!]]", "[^]]", "[][:alpha:]]", "[[:punct:]a]", "[a-z-9]", "[a-b-c-d]", "[a[:digit:]-z]", "[a-[:digit:]]", "!*.env", "{.env}*", "{}*"} {
		t.Run(pattern, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(context.Background(), time.Second)
			defer cancel()
			cmd := exec.CommandContext(ctx, "/bin/bash", "--noprofile", "--norc", "-c", "printf '%s\\n' "+pattern)
			cmd.Dir = root
			cmd.Env = []string{"PATH=/usr/bin:/bin", "LC_ALL=C"}
			out, err := cmd.Output()
			if err != nil {
				t.Fatal(err)
			}
			names := strings.Split(strings.TrimSpace(string(out)), "\n")
			for _, name := range subjects {
				want := slices.Contains(names, name)
				if got := GlobMatch(pattern, name); got != want {
					t.Fatalf("%s matches %s=%t; Bash %q", pattern, name, got, names)
				}
			}
		})
	}
	for _, row := range []struct {
		pattern, subject string
		want             bool
	}{
		{"!*.env", ".npmrc", true}, {"!*.env", "x.env", false}, {"!!*.env", "x.env", true},
		{"{.env}*", ".env", true}, {"{}*", ".env", true}, {"[[:alpha:]]*", "credentials", false},
		{"[a-z]*", "credentials", true}, {"{.env,x}*", ".env", true},
	} {
		if got := apiGlobMatch(row.pattern, row.subject); got != row.want {
			t.Errorf("API %q/%q=%t", row.pattern, row.subject, got)
		}
	}
}
func TestPOSIXProtectedPaths(t *testing.T) {
	for _, p := range []string{"/synthetic/project/.[[:alpha:]]nv", "/synthetic/.aws/[[:alpha:]]*", "/synthetic/.aws/[a-z[:digit:]]*", "/synthetic/project/.e[]n]v", "/synthetic/project/.e[!]]v", "/synthetic/project/.e[^]]v", "/synthetic/.aws/credentia[]l]s"} {
		if !IsSensitive(p, true) {
			t.Errorf("protected class allowed: %s", p)
		}
	}
	for _, p := range []string{"/synthetic/Library/[[:alpha:]]ontainers/x", "/synthetic/Library/[]C]ontainers/x", "/synthetic/Library/[!]]ontainers/x", "/synthetic/Library/[a-z-9]ontainers/x", "/synthetic/Library/[a-é-c]ontainers/x"} {
		if !IsAppdata(p, "/synthetic", true) {
			t.Errorf("App Data class allowed: %s", p)
		}
	}
	for _, p := range []string{"/synthetic/project/!*.env", "/synthetic/project/{.env}*"} {
		if !IsSensitiveAPI(p) {
			t.Errorf("API pattern allowed: %s", p)
		}
	}
}
