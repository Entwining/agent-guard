package core

import (
	"errors"
	"os"
	"strings"
	"syscall"
	"testing"

	"agentguard/native/reasons"
	"agentguard/native/record"
)

func TestProbeSafety(t *testing.T) {
	home := syntheticHome(t)
	req := func(s string) record.Request { return BuildRequest("claude", "bash", home+"/project", s, "", home) }
	t.Run("unresolved-public-suffix", func(t *testing.T) {
		for _, s := range []string{"cat ssh-link/config.$UNSET_VARIABLE", "cat ssh-link/config.${PROFILE}", "cat < ssh-link/config.$UNSET_VARIABLE", "cat > ssh-link/config.${PROFILE}"} {
			p := &watchingProbe{}
			if verdict(t, req(s), p) != "" || len(p.stats) != 0 {
				t.Fatalf("%s: stats %v", s, p.stats)
			}
			for _, path := range p.readlinks {
				if strings.ContainsAny(path, "$`") {
					t.Fatalf("uncertain probe %s", path)
				}
			}
		}
		p := &watchingProbe{}
		if verdict(t, req("cat ssh-link/config"), p) != "" || len(p.stats) == 0 {
			t.Fatal("resolved public control did not probe")
		}
	})
	t.Run("physical-protected-prefix", func(t *testing.T) {
		p := &watchingProbe{}
		if verdict(t, req("cd -P data-link/com.x && cd .. && cd sub && cat x"), p) != reasons.Appdata {
			t.Fatal("physical App Data allowed")
		}
		for _, path := range append(p.readlinks, p.stats...) {
			if strings.HasPrefix(strings.ToLower(path), strings.ToLower(home+"/Library/Containers")) {
				t.Fatalf("protected probe %s", path)
			}
		}
	})
	t.Run("physical-unresolved-prefix", func(t *testing.T) {
		p := &watchingProbe{}
		if verdict(t, req("cd -P cache-link/$UNSET_VARIABLE/.. && cat x"), p) != "" {
			t.Fatal("unresolved cwd unexpectedly denied")
		}
		for _, path := range p.readlinks {
			if strings.Contains(path, "$UNSET_VARIABLE") {
				t.Fatal(path)
			}
		}
	})
	t.Run("unresolved-private-alias", func(t *testing.T) {
		for _, s := range []string{"cat ssh-link/$UNSET_VARIABLE", "cat ssh-link/$UNSET_VARIABLE/known_hosts"} {
			p := &watchingProbe{}
			if verdict(t, req(s), p) != reasons.File || len(p.stats) != 0 {
				t.Fatalf("%s: stats %v", s, p.stats)
			}
			for _, path := range p.readlinks {
				if strings.Contains(path, "$UNSET_VARIABLE") {
					t.Fatal(path)
				}
			}
		}
	})
	t.Run("unresolved-case-alias", func(t *testing.T) {
		for _, s := range []string{"cat .SSH/$UNSET_VARIABLE", "cat ~/.Ssh/${PROFILE}id_rsa", "cat < .SSH/$UNSET_VARIABLE", "cat > .SSH/${PROFILE}id_rsa"} {
			p := &watchingProbe{}
			if verdict(t, BuildRequest("claude", "bash", home, s, "", home), p) != reasons.Ssh || len(p.stats) != 0 {
				t.Fatalf("%s: stats %v", s, p.stats)
			}
			for _, path := range p.readlinks {
				if strings.ContainsAny(path, "$`") {
					t.Fatal(path)
				}
			}
		}
		p := &watchingProbe{}
		if verdict(t, BuildRequest("claude", "bash", home, "cat .SSH/config.$UNSET_VARIABLE", "", home), p) != "" || len(p.stats) != 0 {
			t.Fatal("uncertain public alias must not stat")
		}
	})
	t.Run("linked-glob-kernel-premise", func(t *testing.T) {
		if e := os.Chmod(home+"/Library/Containers/com.x", 0); e != nil {
			t.Fatal(e)
		}
		t.Cleanup(func() { _ = os.Chmod(home+"/Library/Containers/com.x", 0755) })
		for _, c := range []struct {
			path string
			err  error
		}{{home + "/project/data-link/com.x/*.txt", syscall.EACCES}, {home + "/project/cache-link/*.txt", syscall.ENOENT}} {
			_, e := os.Stat(c.path)
			if !errors.Is(e, c.err) {
				t.Fatalf("%s: %v, want %v", c.path, e, c.err)
			}
		}
	})
	t.Run("ssh-link-to-appdata", func(t *testing.T) {
		p := &watchingProbe{}
		if verdict(t, BuildRequest("claude", "bash", home, "ls .ssh/inside-link", "", home), p) != reasons.Ssh {
			t.Fatal("SSH link allowed")
		}
		for _, path := range append(p.stats, p.readlinks...) {
			if strings.HasPrefix(path, home+"/Library/Containers") {
				t.Fatal(path)
			}
		}
	})
	t.Run("inode-error-contract", func(t *testing.T) {
		for _, c := range []struct {
			err    error
			reason string
		}{{syscall.EIO, reasons.Symlink}, {syscall.ENOENT, ""}} {
			p := &watchingProbe{statError: c.err}
			if got := verdict(t, req("cat ~/.ssh/id.pub"), p); got != c.reason {
				t.Fatalf("%v: %q", c.err, got)
			}
		}
	})
}
