package native

import "regexp"

// These patterns retain ECMAScript whitespace rather than Go's Unicode whitespace set.
const SpaceClass = `\x09-\x0d\x20\x{00a0}\x{1680}\x{2000}-\x{200a}\x{2028}\x{2029}\x{202f}\x{205f}\x{3000}\x{feff}`
const Space = "[" + SpaceClass + "]"
const NonSpace = "[^" + SpaceClass + "]"

var splitter = regexp.MustCompile(Space + "+")

func Fields(s string) []string {
	parts := splitter.Split(s, -1)
	if parts[0] == "" {
		parts = parts[1:]
	}
	if len(parts) > 0 && parts[len(parts)-1] == "" {
		parts = parts[:len(parts)-1]
	}
	return parts
}
