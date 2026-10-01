package filesystem

import (
	"strings"
)

func Braces(s string) []string {
	for i := 0; i < len(s); i++ {
		if s[i] == '\\' {
			i++
			continue
		}
		if s[i] != '{' {
			continue
		}
		depth := 1
		start := i + 1
		parts := []string{}
		j := start
		for ; j < len(s); j++ {
			if s[j] == '\\' {
				j++
				continue
			}
			switch s[j] {
			case '{':
				depth++
			case '}':
				depth--
			case ',':
				if depth == 1 {
					parts = append(parts, s[start:j])
					start = j + 1
				}
			}
			if depth == 0 {
				break
			}
		}
		if j == len(s) {
			continue
		}
		if len(parts) == 0 {
			continue
		}
		parts = append(parts, s[start:j])
		r := []string{}
		for _, p := range parts {
			r = append(r, Braces(s[:i]+p+s[j+1:])...)
		}
		return r
	}
	return []string{s}
}

func GlobMatch(p, s string) bool {
	for _, b := range Braces(p) {
		if matchPath(b, s) {
			return true
		}
	}
	return false
}

func globReaches(p, d string) bool {
	a, b := strings.Split(p, "/"), strings.Split(d, "/")
	if len(a) <= len(b) {
		return false
	}
	for i, v := range b {
		if i > 0 && a[i] != "**" && !GlobMatch(strings.ToLower(a[i]), strings.ToLower(v)) {
			return false
		}
	}
	return true
}

type globToken struct {
	star    bool
	pattern string
}

func tokens(s string) []globToken {
	r := []globToken{}
	for i := 0; i < len(s); i++ {
		c := s[i]
		if c == '*' {
			r = append(r, globToken{star: true})
			continue
		}
		if c == '[' && i+2 <= len(s) {
			if end := bracketEnd(s, i); end >= 0 {
				r = append(r, globToken{pattern: s[i : end+1]})
				i = end
				continue
			}
		}
		if c == '\\' && i+1 < len(s) {
			i++
			r = append(r, globToken{pattern: "\\" + string(s[i])})
		} else {
			r = append(r, globToken{pattern: string(c)})
		}
	}
	return r
}

func globsIntersect(a, b string) bool {
	x, y := tokens(a), tokens(b)
	seen := map[[2]int]bool{}
	var walk func(int, int) bool
	walk = func(i, j int) bool {
		k := [2]int{i, j}
		if seen[k] {
			return false
		}
		seen[k] = true
		if i == len(x) && j == len(y) {
			return true
		}
		xs, ys := i < len(x) && x[i].star, j < len(y) && y[j].star
		if xs && walk(i+1, j) {
			return true
		}
		if ys && walk(i, j+1) {
			return true
		}
		if xs && ys {
			return false
		}
		if xs {
			return j < len(y) && walk(i, j+1)
		}
		if ys {
			return i < len(x) && walk(i+1, j)
		}
		if i == len(x) || j == len(y) {
			return false
		}
		for c := 33; c < 127; c++ {
			s := string(rune(c))
			if GlobMatch(x[i].pattern, s) && GlobMatch(y[j].pattern, s) {
				return walk(i+1, j+1)
			}
		}
		return false
	}
	return walk(0, 0)
}
