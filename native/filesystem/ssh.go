package filesystem

import (
	"context"
	"os"
	"path"
	"strings"
)

func stat(ctx context.Context, p string, probe Probe) (os.FileInfo, error) {
	if e := ctx.Err(); e != nil {
		return nil, e
	}
	s, e := probe.Stat(p)
	if canceled := ctx.Err(); canceled != nil {
		return nil, canceled
	}
	if NotALink(e) {
		return nil, nil
	}
	return s, e
}

func sameFile(ctx context.Context, a, b string, exact bool, probe Probe) (bool, error) {
	if !exact {
		return strings.EqualFold(a, b), nil
	}
	if a == b {
		return true, nil
	}
	x, e := stat(ctx, a, probe)
	if e != nil {
		return false, e
	}
	y, e := stat(ctx, b, probe)
	if e != nil {
		return false, e
	}
	return x != nil && y != nil && os.SameFile(x, y), nil
}

func kind(ctx context.Context, p string, exact bool, probe Probe) (string, error) {
	if !exact {
		return "other", nil
	}
	s, e := stat(ctx, p, probe)
	if e != nil {
		return "", e
	}
	if s != nil {
		if s.IsDir() {
			return "dir", nil
		}
		if s.Mode().IsRegular() {
			return "file", nil
		}
	}
	return "other", nil
}

func near(p, root string, search bool) bool {
	s, b := strings.ToLower(strings.TrimSuffix(p, "/")), strings.ToLower(root)
	if s == "" {
		s = "/"
	}
	return s == b || strings.HasPrefix(s, b+"/") || search && (s == "/" || strings.HasPrefix(b, s+"/"))
}

func SSHScopeDenied(ctx context.Context, target, home string, search, exact bool, probe Probe) (bool, error) {
	ssh := home + "/.ssh"
	stop := func(p string) bool { return IsAppdata(p, home) }
	root, e := FollowLinks(ctx, ssh, home, probe, stop)
	if e != nil {
		return false, e
	}
	roots := []string{ssh}
	if root != ssh {
		roots = append(roots, root)
	}
	candidates := []string{target}
	if exact {
		p, e := FollowLinks(ctx, target, home, probe, stop)
		if e != nil {
			return false, e
		}
		if p != target {
			candidates = append(candidates, p)
		}
	}
	inScope := false
	for _, c := range candidates {
		for _, r := range roots {
			inScope = inScope || near(c, r, search)
		}
	}
	if !inScope {
		return false, nil
	}
	for _, p := range append(append([]string{}, roots...), candidates...) {
		if IsAppdata(p, home) {
			return true, nil
		}
	}
	for _, c := range candidates {
		for _, r := range roots {
			same, e := sameFile(ctx, c, r, exact, probe)
			if e != nil || same {
				return same, e
			}
			if search {
				for parent := r; parent != "/"; {
					parent = path.Dir(parent)
					same, e := sameFile(ctx, c, parent, exact, probe)
					if e != nil || same {
						return same, e
					}
				}
			}
			for parent := c; parent != "/"; {
				parent = path.Dir(parent)
				same, e := sameFile(ctx, parent, r, exact, probe)
				if e != nil {
					return false, e
				}
				if !same {
					continue
				}
				if parent != path.Dir(c) || !SSHPublic(path.Base(c)) {
					return true, nil
				}
				k, e := kind(ctx, target, exact, probe)
				if e != nil {
					return false, e
				}
				if k == "dir" || search && k != "file" {
					return true, nil
				}
			}
		}
	}
	return false, nil
}
