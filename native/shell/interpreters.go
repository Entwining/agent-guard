package shell

import (
	"regexp"
	"strings"

	"agentguard/native/record"
)

type interpreter struct {
	code, value string
	glued       bool
}

var interpreters = map[string]interpreter{"python": {"c", "WX", true}, "python3": {"c", "WX", true}, "node": {"ep", "", false}, "bun": {"ep", "", true}, "ruby": {"e", "rICEix", true}, "perl": {"eE", "MmIidDCFx", true}, "php": {"rR", "dcfz", true}, "osascript": {"e", "", true}, "lua": {"e", "l", true}, "deno": {"", "", false}}

func interpreterName(name string) string {
	if _, ok := interpreters[name]; ok {
		return name
	}
	n := regexp.MustCompile(`[\d.]+$`).ReplaceAllString(name, "")
	if _, ok := interpreters[n]; ok {
		return n
	}
	return ""
}

func interpreterCode(name string, args []*record.Word) []string {
	f := interpreters[name]
	found := []string{}
	take := func(w *record.Word) {
		if w != nil {
			w.Role = "code"
			found = append(found, w.Text)
		}
	}
	if name == "deno" {
		ops := []*record.Word{}
		for _, w := range args {
			if !strings.HasPrefix(w.Text, "-") {
				ops = append(ops, w)
			}
		}
		for i, w := range ops {
			if i >= 2 {
				break
			}
			if w.Text == "eval" {
				for _, v := range ops[i+1:] {
					take(v)
				}
				break
			}
		}
		return found
	}
	for i := 0; i < len(args); i++ {
		text := args[i].Text
		p := `(?s)^--(eval|print)(=(.*))?$`
		if name == "php" {
			p = `(?s)^--(eval|print|run)(=(.*))?$`
		}
		if m := match(p, text); m != nil {
			if m[2] == "" {
				i++
				take(record.At(args, i))
			} else {
				found = append(found, m[3])
			}
			continue
		}
		if !rx(`^-[^-]`, text) {
			continue
		}
		for k := 1; k < len(text); k++ {
			if strings.ContainsRune(f.value, rune(text[k])) {
				break
			}
			if strings.ContainsRune(f.code, rune(text[k])) {
				if k < len(text)-1 && !f.glued {
					continue
				}
				if k < len(text)-1 {
					found = append(found, text[k+1:])
				} else {
					i++
					take(record.At(args, i))
				}
				break
			}
		}
	}
	return found
}
