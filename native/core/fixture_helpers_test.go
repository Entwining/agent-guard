package core

import (
	"agentguard/native/filesystem"
	"agentguard/native/record"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"syscall"
	"testing"
)

type watchingProbe struct {
	mu               sync.Mutex
	readlinks, stats []string
	violations       []string
	statError        error
}

func (p *watchingProbe) Readlink(s string) (string, error) {
	p.mu.Lock()
	p.readlinks = append(p.readlinks, s)
	p.mu.Unlock()
	link, e := os.Readlink(s)
	if errors.Is(e, syscall.EACCES) && !strings.Contains(s, "/project/locked") {
		p.mu.Lock()
		p.violations = append(p.violations, "readlink "+s)
		p.mu.Unlock()
	}
	return link, e
}

func (p *watchingProbe) Stat(s string) (os.FileInfo, error) {
	p.mu.Lock()
	p.stats = append(p.stats, s)
	statError := p.statError
	p.mu.Unlock()
	if statError != nil {
		return nil, statError
	}
	info, e := os.Stat(s)
	if errors.Is(e, syscall.EACCES) && !strings.Contains(s, "/project/locked") {
		p.mu.Lock()
		p.violations = append(p.violations, "stat "+s)
		p.mu.Unlock()
	}
	return info, e
}

func syntheticHome(t *testing.T) string {
	t.Helper()
	home, e := filepath.EvalSymlinks(t.TempDir())
	if e != nil {
		t.Fatal(e)
	}
	for _, d := range []string{"project/nested", ".ssh/config.d", ".ssh/directory.pub", "Library/Containers/com.x", "Library/Caches/tool"} {
		if e := os.MkdirAll(home+"/"+d, 0755); e != nil {
			t.Fatal(e)
		}
	}
	for _, f := range []string{".ssh/private", ".ssh/config", ".ssh/id.pub", ".ssh/config.d/private", "project/file.txt"} {
		if e := os.WriteFile(home+"/"+f, nil, 0600); e != nil {
			t.Fatal(e)
		}
	}
	for link, dest := range map[string]string{"project/ssh-link": ".ssh", "project/data-link": "Library/Containers", "project/cache-link": "Library/Caches/tool", ".ssh/inside-link": "Library/Containers/com.x"} {
		if e := os.Symlink(home+"/"+dest, home+"/"+link); e != nil {
			t.Fatal(e)
		}
	}
	return home
}

func verdict(t *testing.T, req record.Request, probe filesystem.Probe) string {
	t.Helper()
	r, e := Evaluate(t.Context(), req, probe)
	if e != nil {
		t.Fatal(e)
	}
	return r
}

func resolveExisting(t *testing.T, path string) string {
	t.Helper()
	resolved, e := filepath.EvalSymlinks(path)
	if e == nil {
		return resolved
	}
	if !errors.Is(e, syscall.ENOENT) || path == "/" {
		t.Fatalf("scope premise failed for %s: %v", path, e)
	}
	return filepath.Join(resolveExisting(t, filepath.Dir(path)), filepath.Base(path))
}
