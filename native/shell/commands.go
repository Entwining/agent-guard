package shell

import (
	"agentguard/native/record"
	"mvdan.cc/sh/v3/syntax"
	"slices"
	"strings"
)

func (f *frontend) expansions(node syntax.Node, s scope, names *[]string) {
	syntax.Walk(node, func(child syntax.Node) bool {
		if child == nil {
			return true
		}
		switch c := child.(type) {
		case *syntax.ParamExp:
			if c.Param != nil {
				*names = append(*names, c.Param.Value)
			}
		case *syntax.CmdSubst:
			inner := isolated(s)
			for _, stmt := range c.Stmts {
				f.statement(stmt, inner)
			}
			return false
		case *syntax.ProcSubst:
			inner := isolated(s)
			for _, stmt := range c.Stmts {
				f.statement(stmt, inner)
			}
			return false
		}
		return true
	})
}

func (f *frontend) assign(node *syntax.Assign, s scope, role string) *record.Word {
	name := ""
	if node.Name != nil {
		name = node.Name.Value
	}
	value := record.Literal("")
	if node.Value != nil {
		value = f.word(node.Value, s)
	}
	syntax.Walk(node, func(child syntax.Node) bool {
		if child == nil || child == node {
			return true
		}
		if child == node.Value {
			return false
		}
		if w, ok := child.(*syntax.Word); ok {
			f.word(w, s)
			return false
		}
		return true
	})
	out := *value
	out.Text = name + "=" + value.Text
	out.Value = out.Text
	out.Raw = text(node, s)
	out.Role = role
	return &out
}

func (f *frontend) simple(node syntax.Node, redirs []*syntax.Redirect, s scope) {
	argv := []*record.Word{}
	switch cmd := node.(type) {
	case *syntax.CallExpr:
		for _, a := range cmd.Assigns {
			argv = append(argv, f.assign(a, s, "assign"))
		}
		for _, w := range cmd.Args {
			argv = append(argv, f.word(w, s))
		}
	case *syntax.DeclClause:
		argv = append(argv, record.Literal(cmd.Variant.Value))
		for _, a := range cmd.Args {
			if !a.Naked {
				argv = append(argv, f.assign(a, s, "arg"))
			} else if a.Value != nil {
				argv = append(argv, f.word(a.Value, s))
			} else if a.Name != nil {
				argv = append(argv, record.Literal(a.Name.Value))
			} else {
				argv = append(argv, record.Literal(""))
			}
		}
	}
	redirects := []record.Redirect{}
	for _, r := range redirs {
		if v := f.redirect(r, s); v != nil {
			redirects = append(redirects, *v)
		}
	}
	command := &record.Command{Argv: argv, Redirects: redirects, Cwd: s.dir.cwd, Program: -1, Wrappers: []string{}, Shell: true}
	f.script.Commands = append(f.script.Commands, command)
	sources, code := resolveCommand(command, f.home)
	for _, cwd := range s.dir.alternatives {
		copy := *command
		copy.Cwd = cwd
		copy.Argv = append([]*record.Word{}, command.Argv...)
		for i, w := range copy.Argv {
			if w.Pwd {
				x := *w
				x.Text = strings.ReplaceAll(x.Text, s.dir.cwd, cwd)
				x.Value = strings.ReplaceAll(x.Value, s.dir.cwd, cwd)
				copy.Argv[i] = &x
			}
		}
		f.script.Commands = append(f.script.Commands, &copy)
	}
	for _, item := range sources {
		for _, cwd := range append([]string{command.Cwd}, s.dir.alternatives...) {
			start := len(f.script.Commands)
			f.parse(item.source, cwd)
			if item.items != nil {
				for _, child := range f.script.Commands[start:] {
					if child.Items == nil {
						child.Items = item.items
					}
				}
			}
		}
	}
	for _, code := range code {
		f.script.Uninspectable = append(f.script.Uninspectable, record.Fragment{Text: code, Cwd: command.Cwd})
	}
	for _, r := range redirects {
		if r.Direction != "heredoc" && r.Direction != "herestring" {
			continue
		}
		switch stdinKind(command) {
		case "shell":
			f.parse(r.Target, command.Cwd)
		case "code":
			f.script.Uninspectable = append(f.script.Uninspectable, record.Fragment{Text: r.Target, Cwd: command.Cwd})
		}
		if slices.Contains(command.Wrappers, "xargs") {
			for _, item := range xargsHereInput(command, r.Target) {
				f.parse(item.source, item.cwd)
			}
		}
	}
	called := ""
	valid := true
	for _, w := range command.Wrappers {
		valid = valid && w == "time"
	}
	if valid {
		called = record.Text(command.Argv, command.Program)
	}
	if fn, ok := f.functions[called]; ok && !f.running[called] {
		f.functionRuns++
		if f.functionRuns > 256 {
			f.script.ParseFailed = true
		} else {
			f.running[called] = true
			failures := s.dir.failures
			s.dir.failures = nil
			bodyScope := s
			bodyScope.src = fn.src
			f.statement(fn.body, bodyScope)
			s.dir.failures = failures
			delete(f.running, called)
		}
	}
	f.track(command, s)
}
