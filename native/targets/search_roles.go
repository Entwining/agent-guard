package targets

import (
	"slices"
	"strings"

	"agentguard/native/record"
)

func ShowsHidden(ws []*record.Word) bool {
	for _, w := range ws {
		if rx(`^(--hidden|--unrestricted)$`, w.Text) || rx(`^-[A-Za-z]*[Hu][A-Za-z]*$`, w.Text) {
			return true
		}
	}
	return false
}

func SearchRoles(cmd *record.Command, args []*record.Word, prog string) {
	rg := prog == "rg"
	short := "efABCmdD"
	long := strings.Fields("regexp file include exclude exclude-dir exclude-from label context after-context before-context max-count binary-files devices directories")
	if rg {
		short = "efgtTEABCmMjrd"
		long = strings.Fields("regexp file glob iglob type type-not encoding replace color colors sort sortr max-depth max-filesize pre pre-glob engine threads max-columns type-add type-clear path-separator context-separator field-context-separator field-match-separator after-context before-context context max-count ignore-file dfa-size-limit regex-size-limit hyperlink-format")
	}
	operands := []*record.Word{}
	options := true
	unrestricted := 0
	noHidden := false
	value := func(at int, key, text string) {
		t := record.At(args, at)
		if t == nil {
			return
		}
		if !rg && (key == "d" || key == "directories") && text == "recurse" {
			cmd.Flags.Add("recursive")
		}
		role := "optarg"
		switch key {
		case "e", "regexp":
			role = "pattern"
			cmd.Flags.Add("explicit")
		case "f", "file":
			role = "patfile"
			cmd.Flags.Add("explicit")
		case "g", "glob", "iglob", "include":
			role = "glob"
			if strings.HasPrefix(text, "!") {
				role = "nglob"
			}
		}
		if t.Role == "option" {
			t.Role = "option:" + role
		} else {
			t.Role = role
		}
		t.Value = text
	}
	for i := 0; i < len(args); i++ {
		word := args[i].Text
		if options && word == "--" {
			options = false
			args[i].Role = "option"
		} else if options && rx(`^--.`, word) {
			args[i].Role = "option"
			key := strings.SplitN(word[2:], "=", 2)[0]
			switch {
			case key == "files":
				cmd.Flags.Add("files")
			case rg && key == "hidden":
				noHidden = false
				cmd.Flags.Add("hidden")
			case rg && key == "no-hidden":
				noHidden = true
				cmd.Flags.Delete("hidden")
			case rg && key == "unrestricted":
				unrestricted++
				if unrestricted >= 2 && !noHidden {
					cmd.Flags.Add("hidden")
				}
			case prog == "ag" && (key == "hidden" || key == "unrestricted"):
				cmd.Flags.Add("hidden")
			case key == "help" || key == "version":
				cmd.Flags.Add("help")
			case key == "fixed-strings":
				cmd.Flags.Add("fixed")
			case key == "recursive":
				cmd.Flags.Add("recursive")
			case key == "include":
				cmd.Flags.Add("include")
			}
			if at := strings.Index(word, "="); at >= 0 {
				value(i, key, word[at+1:])
			} else if slices.Contains(long, key) {
				i++
				value(i, key, record.Text(args, i))
			}
		} else if options && rx(`^-.`, word) {
			args[i].Role = "option"
			for k := 1; k < len(word); k++ {
				ch := word[k]
				if (rg || prog == "grep") && ch == 'F' {
					cmd.Flags.Add("fixed")
				}
				if prog == "grep" && (ch == 'r' || ch == 'R') {
					cmd.Flags.Add("recursive")
				}
				if rg && ch == 'h' || ch == 'V' {
					cmd.Flags.Add("help")
				}
				if rg && ch == 'r' {
					cmd.Flags.Add("replace")
				}
				if rg && ch == '.' {
					cmd.Flags.Add("hidden")
				}
				if rg && ch == 'u' {
					unrestricted++
					if unrestricted >= 2 && !noHidden {
						cmd.Flags.Add("hidden")
					}
				}
				if prog == "ag" && ch == 'u' {
					cmd.Flags.Add("hidden")
				}
				if !strings.ContainsRune(short, rune(ch)) {
					continue
				}
				text := word[k+1:]
				if rg {
					text = strings.TrimPrefix(text, "=")
				}
				if text == "" {
					i++
					text = record.Text(args, i)
				}
				value(i, string(ch), text)
				break
			}
		} else {
			operands = append(operands, args[i])
		}
	}
	explicit := cmd.Flags.Has("explicit") || cmd.Flags.Has("files")
	for _, w := range operands {
		w.Role = "pattern"
		if explicit {
			w.Role = "path"
		}
		explicit = true
	}
}
