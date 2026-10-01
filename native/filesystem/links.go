package filesystem

import (
	"context"
	"fmt"
	"os"
	"path"
	"strings"
)

var firmlinks map[string]bool
var firmlinkError error

func init() {
	b, e := os.ReadFile("/usr/share/firmlinks")
	firmlinkError = e
	firmlinks = map[string]bool{}
	if e == nil {
		for _, line := range strings.Split(string(b), "\n") {
			v := strings.Split(line, "\t")
			if len(v) > 1 {
				firmlinks["/system/volumes/data/"+strings.ToLower(v[1])] = true
			}
		}
	}
}

func InitializationError() error { return firmlinkError }

// Readlink alone examines command-derived paths; the stop predicate runs before every probe.
func FollowLinks(ctx context.Context, absolute, home string, probe Probe, stop func(string) bool) (string, error) {
	if e := ctx.Err(); e != nil {
		return "", e
	}
	if firmlinkError != nil {
		return "", firmlinkError
	}
	if stop == nil {
		stop = func(p string) bool { return IsAppdata(p, home) || IsSensitive(p) }
	}
	p := "/"
	parts := nonempty(strings.Split(absolute, "/"))
	followed := false
	depth := 0
	for len(parts) > 0 {
		if e := ctx.Err(); e != nil {
			return "", e
		}
		part := parts[0]
		parts = parts[1:]
		if part == ".." {
			if firmlinks[strings.ToLower(p)] {
				p = Unfirmlink(p)
			}
			p = path.Dir(p)
		} else {
			p = Resolve(p, part)
		}
		if stop(Unfirmlink(p)) {
			return Unfirmlink(p), nil
		}
		target, e := probe.Readlink(p)
		if canceled := ctx.Err(); canceled != nil {
			return "", canceled
		}
		if e != nil {
			if NotALink(e) {
				continue
			}
			return "", e
		}
		depth++
		if depth > 8 {
			return "", fmt.Errorf("symlink chain exceeds the agent guard limit")
		}
		if !strings.HasPrefix(target, "/") {
			target = path.Dir(p) + "/" + target
		}
		followed = true
		parts = append(nonempty(strings.Split(target, "/")), parts...)
		p = "/"
	}
	if followed {
		return Unfirmlink(p), nil
	}
	return absolute, nil
}

func nonempty(ss []string) []string {
	r := []string{}
	for _, s := range ss {
		if s != "" {
			r = append(r, s)
		}
	}
	return r
}

func followPrefix(ctx context.Context, absolute, home string, probe Probe) (string, error) {
	ss := strings.Split(absolute, "/")
	for i, seg := range ss {
		if !strings.ContainsAny(seg, "*?[{$`") {
			continue
		}
		prefix := strings.Join(ss[:i], "/")
		if prefix == "" {
			prefix = "/"
		}
		p, e := FollowLinks(ctx, prefix, home, probe, nil)
		if e != nil {
			return "", e
		}
		if p == prefix {
			return absolute, nil
		}
		if p == "/" {
			p = ""
		}
		return p + "/" + strings.Join(ss[i:], "/"), nil
	}
	return FollowLinks(ctx, absolute, home, probe, nil)
}
