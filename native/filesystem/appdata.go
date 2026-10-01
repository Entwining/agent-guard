package filesystem

import (
	"strings"
)

var AppdataTrees = []string{"Containers", "Group Containers", "Mobile Documents", "CloudStorage"}

func IsAppdata(p, home string, glob ...bool) bool {
	g := len(glob) > 0 && glob[0]
	if g {
		b := Braces(p)
		if len(b) > 1 {
			for _, s := range b {
				if IsAppdata(s, home, true) {
					return true
				}
			}
			return false
		}
		for _, t := range AppdataTrees {
			if globReaches(p, home+"/Library/"+t) {
				return true
			}
		}
	}
	l := strings.ToLower(home + "/Library/")
	if !strings.HasPrefix(strings.ToLower(p), l) {
		return false
	}
	rest := strings.ToLower(p[len(l):])
	for _, t := range AppdataTrees {
		t = strings.ToLower(t)
		if rest == t || strings.HasPrefix(rest, t+"/") {
			return true
		}
	}
	if g {
		fixed := rest
		if i := strings.IndexAny(fixed, "*?["); i >= 0 {
			fixed = fixed[:i]
		}
		fixed = strings.TrimSuffix(fixed, "/")
		for _, t := range AppdataTrees {
			if fixed != "" && strings.HasPrefix(strings.ToLower(t), fixed) {
				return true
			}
		}
	}
	return false
}

func IsBroad(p, home string, glob ...bool) bool {
	g := len(glob) > 0 && glob[0]
	if g {
		b := Braces(p)
		if len(b) > 1 {
			for _, s := range b {
				if IsBroad(s, home, true) {
					return true
				}
			}
			return false
		}
	}
	p, home = strings.ToLower(p), strings.ToLower(home)
	if p == "/" {
		return true
	}
	trim := strings.TrimSuffix(p, "/")
	if trim == home || trim == home+"/library" || strings.HasPrefix(home, trim+"/") {
		return true
	}
	if !g {
		return false
	}
	candidates := []string{home, home + "/library"}
	for _, t := range AppdataTrees {
		candidates = append(candidates, home+"/library/"+strings.ToLower(t), home+"/library/"+strings.ToLower(t)+"/x")
	}
	for _, c := range candidates {
		if GlobMatch(p, c) {
			return true
		}
	}
	prefix := p
	if i := strings.IndexAny(p, "*?["); i >= 0 {
		prefix = p[:i]
	}
	prefix = strings.TrimSuffix(prefix, "/")
	return strings.Contains(p, "**") && (prefix == home || prefix == home+"/library" || strings.HasPrefix(home, prefix+"/"))
}

func IsLibrary(p, home string) bool {
	return strings.EqualFold(p, home+"/Library") || IsAppdata(p, home)
}
