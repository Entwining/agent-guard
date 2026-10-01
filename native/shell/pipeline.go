package shell

import (
	jsText "agentguard/native"
	"agentguard/native/record"
	"agentguard/native/targets"
	"path"
	"regexp"
	"slices"
	"strconv"
	"strings"
)

func hiddenName(s string) bool {
	n := path.Base(s)
	return strings.HasPrefix(n, ".") && n != "." && n != ".."
}

func markWalkedInput(left, right []*record.Command) {
	var walker *record.Command
	for _, c := range left {
		p := record.At(c.Argv, c.Program)
		if p == nil {
			continue
		}
		name := path.Base(p.Text)
		args := record.Rest(c)
		hit := name == "find" || name == "fd" && targets.ShowsHidden(args)
		for _, w := range args {
			if name == "ls" && (rx(`^(--all|--almost-all|-[A-Za-z0-9]*[aA][A-Za-z0-9]*)$`, w.Text) || !strings.HasPrefix(w.Text, "-") && hiddenName(w.Text)) {
				hit = true
			}
			if (name == "echo" || name == "printf") && !strings.HasPrefix(w.Text, "-") && w.Globs && hiddenName(w.Text) {
				hit = true
			}
		}
		if hit {
			walker = c
			break
		}
	}
	if walker != nil {
		for _, c := range right {
			if slices.Contains(c.Wrappers, "xargs") {
				c.Items = &record.Items{Root: walker.Cwd, Hidden: true}
			}
		}
	}
}

func unescape(text string, operand bool) string {
	text = strings.SplitN(text, `\c`, 2)[0]
	oct := `[0-7]{1,3}`
	if operand {
		oct = `0[0-7]{0,3}|[1-7][0-7]{0,2}`
	}
	re := regexp.MustCompile(`\\(?:x([0-9a-fA-F]{1,2})|(` + oct + `)|([nt\\])|u([0-9a-fA-F]{1,4})|U([0-9a-fA-F]{1,8}))`)
	return re.ReplaceAllStringFunc(text, func(s string) string {
		m := re.FindStringSubmatch(s)
		if m[3] != "" {
			return map[string]string{"n": "\n", "t": "\t", `\`: `\`}[m[3]]
		}
		v := m[1]
		base := 16
		if m[2] != "" {
			v = m[2]
			base = 8
		} else if m[4] != "" {
			v = m[4]
		} else if m[5] != "" {
			v = m[5]
		}
		n, _ := strconv.ParseInt(v, base, 64)
		if n == 0 {
			return "\n"
		}
		return string(rune(n & 65535))
	})
}

func printfOutput(args []*record.Word) (string, bool) {
	if record.Text(args, 0) == "--" {
		args = args[1:]
	}
	if len(args) == 0 {
		return "", true
	}
	format := args[0].Text
	operands := []string{}
	for _, w := range args[1:] {
		operands = append(operands, w.Text)
	}
	re := regexp.MustCompile(`%[sb%]`)
	pieces := []string{}
	at := 0
	for _, m := range re.FindAllStringIndex(format, -1) {
		pieces = append(pieces, format[at:m[0]], format[m[0]:m[1]])
		at = m[1]
	}
	pieces = append(pieces, format[at:])
	converts := false
	for _, p := range pieces {
		if strings.Contains(p, "%") && p != "%s" && p != "%b" && p != "%%" {
			return "", false
		}
		converts = converts || p == "%s" || p == "%b"
	}
	out := ""
	for {
		for _, p := range pieces {
			switch p {
			case "%%":
				out += "%"
			case "%s", "%b":
				v := ""
				if len(operands) > 0 {
					v = operands[0]
					operands = operands[1:]
				}
				if p == "%b" {
					v = unescape(v, true)
				}
				out += v
			default:
				out += unescape(p, false)
			}
		}
		if !converts || len(operands) == 0 {
			break
		}
	}
	return out, true
}

type inputSource struct{ source, cwd string }

func producer(left []*record.Command) *record.Command {
	for _, c := range left {
		p := record.At(c.Argv, c.Program)
		if p != nil && (path.Base(p.Text) == "echo" || path.Base(p.Text) == "printf") {
			return c
		}
	}
	return nil
}

func shellInput(left, right []*record.Command) []inputSource {
	p := producer(left)
	var shell *record.Command
	for _, c := range right {
		if c.Program >= 0 && stdinKind(c) == "shell" {
			shell = c
			break
		}
	}
	if p == nil || shell == nil {
		return nil
	}
	args := record.Rest(p)
	source := ""
	if path.Base(p.Argv[p.Program].Text) == "printf" {
		s, ok := printfOutput(args)
		if !ok {
			return nil
		}
		source = s
	} else {
		ss := []string{}
		for _, w := range args {
			if !strings.HasPrefix(w.Text, "-") {
				ss = append(ss, w.Text)
			}
		}
		source = unescape(strings.Join(ss, " "), true)
	}
	return []inputSource{{source, shell.Cwd}}
}

func xargsCommands(c *record.Command, items []string) []inputSource {
	options := c.Argv[:c.Program]
	marker := ""
	for i, w := range options {
		if w.Text == "-I" || w.Text == "--replace" {
			marker = record.Text(options, i+1)
		} else if strings.HasPrefix(w.Text, "-I") {
			marker = w.Text[2:]
		} else if strings.HasPrefix(w.Text, "--replace=") {
			marker = strings.TrimPrefix(w.Text, "--replace=")
		}
	}
	strip := func(s string) string { return strings.NewReplacer(`"`, "", "'", "", `\`, "").Replace(s) }
	quote := func(s string) string { return "'" + strings.ReplaceAll(s, "'", "'\\''") + "'" }
	command := c.Argv[c.Program:]
	if marker == "" {
		ss := []string{rawJoin(command)}
		for _, item := range items {
			for _, v := range jsText.Fields(item) {
				ss = append(ss, quote(v), quote(strip(v)))
			}
		}
		return []inputSource{{strings.Join(ss, " "), c.Cwd}}
	}
	r := []inputSource{}
	for _, line := range items {
		for _, v := range []string{line, strip(line)} {
			ss := []string{}
			for _, w := range command {
				if strings.Contains(w.Text, marker) {
					ss = append(ss, quote(strings.ReplaceAll(w.Text, marker, v)))
				} else {
					ss = append(ss, w.Raw)
				}
			}
			r = append(r, inputSource{strings.Join(ss, " "), c.Cwd})
		}
	}
	return r
}

func xargsReplacements(left, right []*record.Command) []inputSource {
	p := producer(left)
	var x *record.Command
	for _, c := range right {
		if c.Program >= 0 && slices.Contains(c.Wrappers, "xargs") {
			x = c
			break
		}
	}
	if p == nil || x == nil {
		return nil
	}
	args := record.Rest(p)
	items := []string{}
	if path.Base(p.Argv[p.Program].Text) != "printf" {
		for _, w := range args {
			if !strings.HasPrefix(w.Text, "-") {
				items = append(items, unescape(w.Text, true))
			}
		}
	} else {
		out, ok := printfOutput(args)
		if !ok {
			return nil
		}
		for _, s := range strings.Split(out, "\n") {
			if s != "" {
				items = append(items, s)
			}
		}
	}
	return xargsCommands(x, items)
}

func xargsHereInput(c *record.Command, body string) []inputSource {
	return xargsCommands(c, jsText.Fields(body))
}
