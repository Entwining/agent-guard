package filesystem

import (
	"path"
	"strings"
	"unicode/utf8"
)

// POSIX classes are expanded inside a bracket expression, including mixed and
// negated classes. Keep this syntax at the path matcher, not at policy callers.
var posixClasses = map[string]string{
	"alnum": "a-zA-Z0-9", "alpha": "a-zA-Z", "ascii": "\x00-\x7f",
	"blank": " \t", "cntrl": "\x00-\x1f\x7f", "digit": "0-9",
	"graph": "\x21-\x7e", "lower": "a-z", "print": "\x20-\x7e",
	"punct": "!-/:-@[-`{-~", "space": " \t\r\n\v\f", "upper": "A-Z",
	"word": "a-zA-Z0-9_", "xdigit": "a-fA-F0-9",
}

func bracketPattern(s string) string {
	var out strings.Builder
	for i := 0; i < len(s); i++ {
		c := s[i]
		if c == '\\' && i+1 < len(s) {
			out.WriteString(s[i : i+2])
			i++
			continue
		}
		if c != '[' {
			out.WriteByte(c)
			continue
		}
		end := bracketEnd(s, i)
		if end < 0 {
			out.WriteString(s[i:])
			break
		}
		out.WriteByte('[')
		start := i + 1
		if s[start] == '!' || s[start] == '^' {
			out.WriteByte('^')
			start++
		}
		canStartRange, rangeEndpoint := false, false
		for j := start; j < end; j++ {
			if s[j] == '\\' && j+1 < end {
				_, size := utf8.DecodeRuneInString(s[j+1 : end])
				out.WriteString(s[j : j+1+size])
				j += size
			} else {
				if strings.HasPrefix(s[j:], "[:") {
					if close := strings.Index(s[j+2:end], ":]"); close >= 0 {
						name := s[j+2 : j+2+close]
						if members, ok := posixClasses[name]; ok {
							out.WriteString(members)
							j += close + 3
							canStartRange, rangeEndpoint = false, false
							continue
						}
					}
				}
				if s[j] == '-' && canStartRange && j < end-1 {
					out.WriteByte('-')
					canStartRange, rangeEndpoint = false, true
					continue
				}
				// Bash permits literal - after a range or class; path.Match requires an escape.
				if s[j] == '-' || s[j] == ']' && j == start {
					out.WriteByte('\\')
				}
				_, size := utf8.DecodeRuneInString(s[j:end])
				out.WriteString(s[j : j+size])
				j += size - 1
			}
			if rangeEndpoint {
				canStartRange, rangeEndpoint = false, false
			} else {
				canStartRange = true
			}
		}
		out.WriteByte(']')
		i = end
	}
	return out.String()
}

func matchPath(pattern, subject string) bool {
	// A malformed shell pattern remains literal. It is a syntax partition, not an
	// operational error. path.Match has no filesystem effects or regex compiler.
	p, s := strings.Split(pattern, "/"), strings.Split(subject, "/")
	seen := map[[2]int]bool{}
	var match func(int, int) bool
	match = func(i, j int) bool {
		key := [2]int{i, j}
		if seen[key] {
			return false
		}
		seen[key] = true
		if i == len(p) {
			return j == len(s)
		}
		if p[i] == "**" {
			if match(i+1, j) {
				return true
			}
			return j < len(s) && match(i, j+1)
		}
		if j == len(s) {
			return false
		}
		// Double stars inside a component retain wildcard behavior without crossing
		// an unmentioned path separator.
		component := bracketPattern(p[i])
		ok, err := path.Match(component, s[j])
		if err != nil {
			ok = p[i] == s[j]
		}
		return ok && match(i+1, j+1)
	}
	return match(0, 0)
}

func apiGlobMatch(pattern, subject string) bool {
	negate := false
	for strings.HasPrefix(pattern, "!") {
		negate = !negate
		pattern = pattern[1:]
	}
	matched := false
	for _, candidate := range Braces(pattern) {
		// Bun.Glob consumes single-element braces at this API boundary.
		for {
			left := strings.LastIndex(candidate, "{")
			if left < 0 {
				break
			}
			right := strings.Index(candidate[left:], "}")
			if right < 0 {
				break
			}
			candidate = candidate[:left] + candidate[left+1:left+right] + candidate[left+right+1:]
		}
		if !strings.Contains(candidate, "[:") {
			matched = matched || GlobMatch(candidate, subject)
		}
	}
	return matched != negate
}

func bracketEnd(s string, start int) int {
	i := start + 1
	if i < len(s) && (s[i] == '!' || s[i] == '^') {
		i++
	}
	if i < len(s) && s[i] == ']' {
		i++
	}
	for ; i < len(s); i++ {
		if s[i] == '\\' {
			i++
			continue
		}
		if strings.HasPrefix(s[i:], "[:") {
			if end := strings.Index(s[i+2:], ":]"); end >= 0 {
				i += end + 3
				continue
			}
		}
		if s[i] == ']' {
			return i
		}
	}
	return -1
}
