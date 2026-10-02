package shell

import (
	"path"
	"slices"
	"strings"

	"agentguard/native/filesystem"
	"agentguard/native/record"
)

func boundedDirectories(current string, candidates []string, home string) []string {
	r := []string{}
	seen := map[string]bool{current: true}
	for _, p := range candidates {
		if !seen[p] {
			seen[p] = true
			r = append(r, p)
		}
	}
	if len(r) > 16 {
		return []string{home, home + "/Library"}
	}
	return r
}

func changeDirectory(cwd, target string, physical bool) string {
	if strings.HasPrefix(target, "/") {
		if physical {
			return target + "/."
		}
		return path.Clean(target)
	}
	marker := strings.LastIndex(cwd, "/./")
	if strings.HasSuffix(cwd, "/.") {
		marker = len(cwd) - 2
	}
	if marker < 0 {
		if physical {
			return cwd + "/" + target + "/."
		}
		return filesystem.Resolve(cwd, target)
	}
	raw := cwd[:marker]
	tail := ""
	if marker+3 < len(cwd) {
		tail = cwd[marker+3:]
	}
	if physical {
		if tail != "" {
			tail += "/"
		}
		return raw + "/" + tail + target + "/."
	}
	steps := target
	if tail != "" {
		steps = tail + "/" + target
	}
	ss := strings.Split(path.Clean(steps), "/")
	parents := 0
	for parents < len(ss) && ss[parents] == ".." {
		parents++
	}
	rest := []string{}
	for _, s := range ss[parents:] {
		if s != "." {
			rest = append(rest, s)
		}
	}
	out := raw + strings.Repeat("/..", parents) + "/."
	if len(rest) > 0 {
		out += "/" + strings.Join(rest, "/")
	}
	return out
}

func (f *frontend) track(c *record.Command, s scope) {
	program := record.At(c.Argv, c.Program)
	declaration := program != nil && c.Shell && slices.Contains([]string{"export", "local", "declare", "typeset"}, program.Text)
	for _, w := range c.Argv {
		if (c.Program < 0 && w.Role == "precommand" || declaration && w.Role == "arg") && rx(`^[A-Za-z_][A-Za-z0-9_]*=`, w.Text) && !w.Expands {
			at := strings.Index(w.Text, "=")
			s.vars[w.Text[:at]] = filesystem.ExpandHome(w.Text[at+1:], f.home)
		}
	}
	if program == nil || !c.Shell || (program.Text != "cd" && program.Text != "pushd") {
		return
	}
	args := record.Rest(c)
	operand := -1
	for i, w := range args {
		if !strings.HasPrefix(w.Text, "-") {
			operand = i
			break
		}
	}
	target := ""
	has := operand >= 0
	if has {
		target = args[operand].Text
	} else if program.Text == "cd" {
		has = true
		for _, w := range args {
			has = has && rx(`^(--|-[PLqs]+)$`, w.Text)
		}
		target = f.home
	}
	if !has {
		return
	}
	end := operand
	if end < 0 {
		end = len(args)
	}
	modes := []byte{}
	for _, w := range args[:end] {
		if rx(`^-[A-Za-z]+$`, w.Text) {
			for _, ch := range []byte(w.Text) {
				if ch == 'L' || ch == 'P' {
					modes = append(modes, ch)
				}
			}
		}
	}
	physical := false
	for _, ch := range modes {
		physical = physical || ch == 'P'
	}
	if operand >= 0 {
		physical = physical && !args[operand].Expands && !args[operand].Globs
	}
	disputed := physical && len(modes) > 0 && modes[len(modes)-1] == 'L'
	move := func(cwd string) []string {
		r := []string{changeDirectory(cwd, target, physical)}
		if disputed {
			r = append(r, changeDirectory(cwd, target, false))
		}
		return r
	}
	nexts := move(s.dir.cwd)
	next := nexts[0]
	stayed := append([]string{s.dir.cwd}, s.dir.alternatives...)
	candidates := []string{}
	if s.dir.failures != nil {
		*s.dir.failures = append(*s.dir.failures, stayed...)
	} else {
		candidates = append(candidates, stayed...)
	}
	candidates = append(candidates, nexts[1:]...)
	for _, cwd := range s.dir.alternatives {
		candidates = append(candidates, move(cwd)...)
	}
	s.dir.alternatives = boundedDirectories(next, candidates, f.home)
	s.dir.cwd = next
}
