package shell

import (
	"regexp"
	"strconv"
	"strings"

	"mvdan.cc/sh/v3/syntax"

	jsText "agentguard/native"
	"agentguard/native/filesystem"
	"agentguard/native/record"
)

var braceSequence = regexp.MustCompile(`\{(?:-?\d+|[A-Za-z])\.\.(?:-?\d+|[A-Za-z])(?:\.\.-?\d+)?\}`)
var ansiEscape = regexp.MustCompile(`(?s)\\(?:x([0-9a-fA-F]{1,2})|u([0-9a-fA-F]{4})|([0-7]{1,3})|(.))`)
var quotedEscape = regexp.MustCompile("\\\\([$`\"\\\\])")

func (f *frontend) word(node *syntax.Word, s scope) *record.Word {
	out := &record.Word{Raw: text(node, s), Role: "arg", Vars: []string{}}
	expansion := func(part syntax.Node) {
		name := ""
		plain := false
		if p, ok := part.(*syntax.ParamExp); ok {
			plain = !(p.Excl || p.Length || p.Width || p.Index != nil || p.Slice != nil || p.Repl != nil || p.Exp != nil)
			if plain && p.Param != nil {
				name = p.Param.Value
			}
		}
		printsPwd := name == "PWD"
		if _, ok := part.(*syntax.CmdSubst); ok {
			printsPwd = printsPwd || rx("^(\\$\\(|`)"+jsText.Space+"*pwd("+jsText.Space+"+-[LP])?"+jsText.Space+"*(\\)|`)$", text(part, s))
		}
		known := ""
		ok := false
		if printsPwd {
			known = s.dir.cwd
			ok = true
		} else if name == "HOME" {
			known = f.home
			ok = true
		} else if plain {
			known, ok = s.vars[name]
		}
		out.Pwd = out.Pwd || printsPwd
		if ok {
			out.Text += known
		} else {
			out.Text += text(part, s)
			out.Expands = true
		}
		f.expansions(part, s, &out.Vars)
	}
	for index, part := range node.Parts {
		switch p := part.(type) {
		case *syntax.Lit:
			value := p.Value
			if index == 0 && (value == "~+" || strings.HasPrefix(value, "~+/")) {
				value = s.dir.cwd + value[2:]
				out.Pwd = true
			} else if index == 0 {
				value = filesystem.ExpandHome(value, f.home)
			}
			matches := braceSequence.FindAllStringIndex(value, -1)
			for i := len(matches) - 1; i >= 0; i-- {
				at := matches[i]
				if at[0] == 0 || value[at[0]-1] != '\\' {
					value = value[:at[0]] + "*" + value[at[1]:]
					out.Globs = true
				}
			}
			if len(filesystem.Braces(value)) > 1 {
				out.Globs = true
			}
			for i := 0; i < len(value); i++ {
				if value[i] == '\\' {
					i++
					if i < len(value) {
						out.Text += value[i : i+1]
					}
					continue
				}
				if strings.ContainsRune("*?[", rune(value[i])) {
					out.Globs = true
				}
				out.Text += value[i : i+1]
			}
		case *syntax.SglQuoted:
			if !p.Dollar {
				out.Text += p.Value
			} else {
				out.Text += ansiEscape.ReplaceAllStringFunc(p.Value, func(t string) string {
					m := ansiEscape.FindStringSubmatch(t)
					v := m[1]
					base := 16
					if m[2] != "" {
						v = m[2]
					} else if m[3] != "" {
						v = m[3]
						base = 8
					}
					if v != "" {
						n, _ := strconv.ParseInt(v, base, 32)
						return string(rune(n))
					}
					if e, ok := map[string]string{"a": "\a", "b": "\b", "e": "\x1b", "f": "\f", "n": "\n", "r": "\r", "t": "\t", "v": "\v"}[m[4]]; ok {
						return e
					}
					return m[4]
				})
			}
		case *syntax.DblQuoted:
			for _, inner := range p.Parts {
				if l, ok := inner.(*syntax.Lit); ok {
					v := strings.ReplaceAll(l.Value, "\\\n", "")
					out.Text += quotedEscape.ReplaceAllString(v, "$1")
				} else {
					expansion(inner)
				}
			}
		case *syntax.ExtGlob:
			out.Globs = true
			out.Text += text(part, s)
		default:
			expansion(part)
		}
	}
	out.Value = out.Text
	return out
}
