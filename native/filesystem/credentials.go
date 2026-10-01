package filesystem

import (
	"path"
	"strings"
)

var sensitivePaths = []string{"**/.env", "**/.env.*", "**/.npmrc", "**/.zprofile*", "**/.zsh_history*", "**/*.pem", "**/*.key", "**/auth.json*", "**/.credentials.json*", "**/.aws/credentials*", "**/.netrc", "**/.git-credentials", "**/.docker/config.json", "**/.kube/config", "**/.pypirc", "**/.pgpass", "**/.cargo/credentials*", "**/.config/gh/hosts.yml", "**/private-keys-v1.d", "**/private-keys-v1.d/**"}
var credentialRoots = []string{".aws", ".gnupg"}
var credentialDirectories = []string{".ssh", ".aws", ".gnupg"}
var listedDirectories = func() []string {
	r := []string{}
	for _, p := range sensitivePaths {
		t := strings.Split(strings.TrimPrefix(p, "**/"), "/")
		if len(t) > 1 {
			d := strings.Join(t[:len(t)-1], "/")
			if !strings.Contains(d, "*") {
				r = append(r, d)
			}
		}
	}
	return r
}()

func SSHPublic(s string) bool {
	return s == "config" || strings.HasPrefix(s, "config.") || strings.HasSuffix(s, ".pub") || s == "allowed_signers" || strings.HasPrefix(s, "known_hosts")
}

func SSHPrivate(p string) bool {
	i := strings.LastIndex(p, "/.ssh/")
	if i < 0 || i+6 >= len(p) {
		return false
	}
	rest := p[i+6:]
	return strings.Contains(rest, "/") || !SSHPublic(rest)
}

func IsSensitive(p string, glob ...bool) bool {
	return sensitive(p, len(glob) > 0 && glob[0], false)
}

func IsSensitiveAPI(p string) bool { return sensitive(p, true, true) }

func sensitive(p string, g, api bool) bool {
	match := GlobMatch
	if api {
		match = apiGlobMatch
	}
	bs := Braces(p)
	if len(bs) > 1 {
		for _, b := range bs {
			if sensitive(b, g, api) {
				return true
			}
		}
		return false
	}
	lower := strings.ToLower(p)
	base := path.Base(lower)
	if base == ".env.example" || base == ".env.age" {
		return false
	}
	if SSHPrivate(p) {
		return true
	}
	for _, listed := range sensitivePaths {
		if match(listed, lower) {
			return true
		}
	}
	if !g {
		return false
	}
	segments := strings.Split(p, "/")
	for i, seg := range segments[:len(segments)-1] {
		if !strings.HasPrefix(seg, ".") || !strings.ContainsAny(seg, "*?[") {
			continue
		}
		for _, dir := range credentialDirectories {
			if match(strings.ToLower(seg), dir) {
				ss := append([]string{}, segments...)
				ss[i] = dir
				if sensitive(strings.Join(ss, "/"), true, api) {
					return true
				}
			}
		}
	}
	if strings.Trim(base, "*?") == "" {
		for _, dir := range credentialDirectories {
			if path.Base(path.Dir(lower)) == dir {
				return true
			}
		}
		for _, dir := range listedDirectories {
			if strings.HasSuffix(path.Dir(lower), "/"+dir) {
				return true
			}
		}
		return false
	}
	ss := strings.Split(lower, "/")
	for _, listed := range sensitivePaths {
		tail := strings.Split(strings.TrimPrefix(listed, "**/"), "/")
		if len(tail) > len(ss) || strings.Trim(tail[len(tail)-1], "*") == "" {
			continue
		}
		off := len(ss) - len(tail)
		ok := true
		for i, part := range tail[:len(tail)-1] {
			if !match(ss[off+i], strings.ReplaceAll(part, "*", "x")) {
				ok = false
				break
			}
		}
		if !ok {
			continue
		}
		if len(tail) > 1 {
			if globsIntersect(ss[len(ss)-1], tail[len(tail)-1]) {
				return true
			}
		} else if match(ss[len(ss)-1], strings.ReplaceAll(tail[0], "*", "x")) {
			return true
		}
	}
	return false
}

func IsSensitiveRoot(p, home string) bool {
	l := strings.ToLower(p)
	if IsSensitive(p) {
		return true
	}
	for _, dir := range credentialRoots {
		if path.Base(l) == dir {
			return true
		}
	}
	for _, dir := range listedDirectories {
		if strings.HasSuffix(l, "/"+dir) {
			return true
		}
		parts := strings.Split(dir, "/")
		for i := 1; i < len(parts); i++ {
			if l == strings.ToLower(home)+"/"+strings.Join(parts[:i], "/") {
				return true
			}
		}
	}
	return false
}
