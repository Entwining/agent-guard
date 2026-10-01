package core

import (
	"agentguard/native/filesystem"
	"agentguard/native/reasons"
	"agentguard/native/record"
	"agentguard/native/targets"
	"fmt"
	"os"
	"reflect"
	"strings"
	"testing"
	"time"
)

func TestCoreContracts(t *testing.T) {
	home := syntheticHome(t)
	req := func(s string) record.Request { return BuildRequest("claude", "bash", home+"/project", s, "", home) }
	t.Run("file-url", func(t *testing.T) {
		for _, c := range []struct{ path, reason string }{{"Library/Containers/x", reasons.Appdata}, {".ssh/id_rsa", reasons.File}, {"project/file.txt", ""}} {
			if got := verdict(t, req("curl -s file://"+home+"/"+c.path), filesystem.DiskProbe{}); got != c.reason {
				t.Fatalf("%s: %q", c.path, got)
			}
		}
	})
	t.Run("security-before-advice", func(t *testing.T) {
		safe, unsafe := req("rg -rn foo src"), BuildRequest("claude", "bash", home, "rg -rn foo ~/Library", "", home)
		if verdict(t, safe, filesystem.DiskProbe{}) != "" || verdict(t, unsafe, filesystem.DiskProbe{}) != reasons.Broad {
			t.Fatal("security verdict changed")
		}
		if !reflect.DeepEqual(Suggestions(safe), []string{reasons.Replace}) || !reflect.DeepEqual(Suggestions(unsafe), []string{reasons.Replace}) {
			t.Fatal("advice changed")
		}
	})
	t.Run("syntax", func(t *testing.T) {
		if verdict(t, req("for x (a b); do ls; done"), filesystem.DiskProbe{}) != reasons.Syntax {
			t.Fatal("invalid syntax allowed")
		}
	})
	t.Run("function-budget", func(t *testing.T) {
		if verdict(t, req("f() { :; }; f; f"), filesystem.DiskProbe{}) != "" {
			t.Fatal("ordinary repeated function rejected")
		}
		if verdict(t, req("f() { :; }; "+strings.Repeat("f; ", 257)), filesystem.DiskProbe{}) != reasons.Syntax {
			t.Fatal("function expansion budget was not enforced")
		}
	})
	t.Run("lexical-before-probe", func(t *testing.T) {
		for _, s := range []string{"cat ~/Library/Containers/x", "cat .env"} {
			p := &watchingProbe{}
			if verdict(t, req(s), p) == "" || len(p.readlinks) != 0 || len(p.stats) != 0 {
				t.Fatalf("lexical denial reached probes: %s: %v %v", s, p.readlinks, p.stats)
			}
		}
	})
	t.Run("self-contained-scan", func(t *testing.T) {
		for _, command := range []string{"rg needle", "fd needle"} {
			if verdict(t, BuildRequest("claude", "bash", home, command, "", home), filesystem.DiskProbe{}) != reasons.Broad {
				t.Fatal(command)
			}
		}
	})
	t.Run("bounded-cwd", func(t *testing.T) {
		var input strings.Builder
		for i := 1; i <= 22; i++ {
			fmt.Fprintf(&input, "cd d%d; ", i)
		}
		input.WriteString("du -sh")
		start := time.Now()
		if verdict(t, BuildRequest("claude", "bash", home, input.String(), "", home), filesystem.DiskProbe{}) == "" {
			t.Fatal("ambiguous home walk allowed")
		}
		if time.Since(start) >= 2*time.Second {
			t.Fatal("cwd ambiguity exceeded deadline")
		}
	})
	t.Run("conditional-cwd", func(t *testing.T) {
		for _, c := range []struct{ input, reason string }{
			{`if cd safe; then cd ~/Library/Containers; else cd project; fi; cat x`, reasons.Appdata},
			{`if cd safe; then cd nested; else cd project; fi; cat x`, ""},
			{`if true; then cd ~/Library/Containers; else cat x; fi`, ""},
		} {
			if got := verdict(t, req(c.input), filesystem.DiskProbe{}); got != c.reason {
				t.Fatalf("conditional path: %q, want %q", got, c.reason)
			}
		}
	})
	t.Run("ssh-case", func(t *testing.T) {
		if _, e := os.Stat(home + "/.SSH/private"); e != nil {
			t.Fatal("case-insensitive volume premise:", e)
		}
		for _, command := range []string{"cat .SSH/private", "ls .SSH/private"} {
			if verdict(t, BuildRequest("claude", "bash", home, command, "", home), filesystem.DiskProbe{}) != reasons.Ssh {
				t.Fatal(command)
			}
		}
		for _, tool := range []string{"read", "grep"} {
			if verdict(t, BuildRequest("claude", tool, home, ".SSH/config", "", home), filesystem.DiskProbe{}) != "" {
				t.Fatal("public case alias denied")
			}
		}
		if verdict(t, BuildRequest("claude", "grep", home, ".SSH/private", "", home), filesystem.DiskProbe{}) != reasons.GrepSsh {
			t.Fatal("search case alias allowed")
		}
	})
	t.Run("ssh-relocation", func(t *testing.T) {
		linked := t.TempDir()
		moved := t.TempDir()
		if e := os.WriteFile(moved+"/private", nil, 0600); e != nil {
			t.Fatal(e)
		}
		if e := os.Symlink(moved, linked+"/.ssh"); e != nil {
			t.Fatal(e)
		}
		if verdict(t, BuildRequest("claude", "bash", linked, "cat "+moved+"/private", "", linked), filesystem.DiskProbe{}) != reasons.Ssh {
			t.Fatal("relocated private file allowed")
		}
	})
	t.Run("ssh-client-use", func(t *testing.T) {
		for _, program := range []string{"ssh", "sftp"} {
			ts := targets.ExtractTargets(req(program + " -S " + home + "/.ssh/control.sock host"))
			found := false
			for _, target := range ts {
				found = found || target.Path == home+"/.ssh/control.sock" && target.Effect == "use" && target.Via == "option"
			}
			if !found {
				t.Fatalf("%s: %#v", program, ts)
			}
		}
	})
}
