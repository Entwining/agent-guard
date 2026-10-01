package shell

import (
	"agentguard/native/record"
	"path"
	"regexp"
	"slices"
	"strings"
)

func rx(p, s string) bool { return regexp.MustCompile(p).MatchString(s) }

func match(p, s string) []string { return regexp.MustCompile(p).FindStringSubmatch(s) }

var shellPrograms = strings.Fields("sh bash zsh dash ksh csh tcsh")
var zshBuiltins = strings.Fields("echo printf print export typeset declare set command eval source .")

const shellCodeFlag = `^-[a-z]*c[a-z]*$`

type child struct {
	source string
	items  *record.Items
}

func resolveCommand(cmd *record.Command, home string) ([]child, []string) {
	w := cmd.Argv
	children := []child{}
	code := []string{}
	i := 0
	shell := true
	last := ""
	text := func() string { return record.Text(w, i) }
	mark := func() {
		if t := record.At(w, i); t != nil {
			t.Role = "precommand"
		}
		i++
	}
	assignment := func(t *record.Word) bool { return t.Role == "assign" || rx(`^[A-Za-z_][A-Za-z0-9_]*=`, t.Raw) }
	for i < len(w) && (assignment(w[i]) || w[i].Raw == "nocorrect") {
		mark()
	}
	for i < len(w) && slices.Contains([]string{"builtin", "-", "noglob"}, text()) {
		last = text()
		mark()
	}
	if last == "builtin" && !slices.Contains(zshBuiltins, text()) {
		i = len(w)
	}
wrappers:
	for i < len(w) {
		name := path.Base(text())
		switch name {
		case "command":
			if text() != "command" {
				break wrappers
			}
			mark()
			for text() == "-p" || text() == "--" {
				i++
			}
			if text() == "-v" || text() == "-V" {
				i = len(w)
			}
		case "exec":
			if !shell || text() != "exec" {
				break wrappers
			}
			mark()
			for text() == "-c" || text() == "-l" {
				i++
			}
			if text() == "-a" {
				i += 2
			}
		case "nohup":
			mark()
			if text() == "--" {
				i++
			}
		case "timeout":
			mark()
			for strings.HasPrefix(text(), "-") {
				if slices.Contains([]string{"-s", "--signal", "-k", "--kill-after"}, text()) {
					i++
				}
				i++
			}
			i++
		case "nice":
			mark()
			if text() == "-n" || text() == "--adjustment" {
				i += 2
			} else if rx(`^(-n|--adjustment=|-\d)`, text()) {
				i++
			}
		case "sudo", "doas":
			mark()
			for i < len(w) && text() != "--" && (strings.HasPrefix(text(), "-") || rx(`^[A-Za-z_][A-Za-z0-9_]*=`, text())) {
				if rx(`^(-[A-Za-z]*[ughpCDRTrtU]|--(user|group|host|prompt|chdir|chroot|role|type|other-user|close-from|command-timeout))$`, text()) {
					mark()
				}
				mark()
			}
			if text() == "--" {
				mark()
			}
		case "script":
			mark()
			for strings.HasPrefix(text(), "-") {
				if text() == "-F" || text() == "-t" {
					i++
				}
				i++
			}
			if i < len(w) {
				mark()
			}
		case "arch":
			mark()
			for strings.HasPrefix(text(), "-") {
				if slices.Contains([]string{"-e", "-d", "-arch"}, text()) {
					i++
				}
				i++
			}
		case "stdbuf":
			mark()
			for rx(`^-[ioe]`, text()) {
				if rx(`^-[ioe]$`, text()) {
					i++
				}
				i++
			}
		case "caffeinate":
			mark()
			for rx(`^-[disumtw]$`, text()) {
				if text() == "-t" || text() == "-w" {
					i++
				}
				i++
			}
		case "time":
			mark()
			for strings.HasPrefix(text(), "-") {
				if slices.Contains([]string{"-f", "-o", "--format", "--output"}, text()) {
					i++
				}
				i++
			}
		case "xargs":
			mark()
			for strings.HasPrefix(text(), "-") {
				if slices.Contains(strings.Fields("-a -d -E -I -L -n -P -s --arg-file --delimiter --replace --max-args"), text()) {
					i++
				}
				i++
			}
		case "env":
			mark()
			for i < len(w) {
				arg := text()
				if arg == "-C" {
					if v := record.At(w, i+1); v != nil {
						cmd.Cwd = changeDirectory(cmd.Cwd, v.Text, !v.Expands && !v.Globs)
						v.Role = "precommand"
					}
					i += 2
				} else if arg == "-u" || arg == "-P" {
					i += 2
				} else if arg == "-S" {
					children = append(children, child{source: record.Text(w, i+1)})
					cmd.Wrappers = append(cmd.Wrappers, "env-S")
					i = len(w)
				} else if strings.HasPrefix(arg, "-") || strings.Contains(arg, "=") {
					i++
				} else {
					break
				}
			}
		case "repeat":
			if !shell || text() != "repeat" {
				break wrappers
			}
			mark()
			i++
			continue
		case "envchain":
			mark()
			if strings.HasPrefix(text(), "-") {
				i = len(w)
			}
			if v := record.At(w, i); v != nil {
				v.Role = "namespace"
			}
			i++
		default:
			break wrappers
		}
		cmd.Wrappers = append(cmd.Wrappers, name)
		shell = false
	}
	cmd.Shell = shell
	if i >= len(w) {
		return children, code
	}
	cmd.Program = i
	w[i].Role = "program"
	if strings.HasPrefix(w[i].Text, "=") && len(w[i].Text) > 1 {
		w[i].Text = w[i].Text[1:]
	}
	resolved, fragments := commandSources(cmd, home)
	return append(children, resolved...), append(code, fragments...)
}

func rawJoin(ws []*record.Word) string {
	ss := []string{}
	for _, w := range ws {
		ss = append(ss, w.Raw)
	}
	return strings.Join(ss, " ")
}

func stdinKind(c *record.Command) string {
	p := record.At(c.Argv, c.Program)
	if p == nil {
		return ""
	}
	n := path.Base(p.Text)
	if slices.Contains(shellPrograms, n) {
		has := false
		for _, a := range record.Rest(c) {
			has = has || rx(shellCodeFlag, a.Text)
		}
		if !has {
			return "shell"
		}
	}
	if interpreterName(n) != "" {
		return "code"
	}
	return ""
}
