package shell

import (
	"mvdan.cc/sh/v3/syntax"

	"agentguard/native/record"
)

func (f *frontend) statement(stmt *syntax.Stmt, outer scope) {
	if stmt == nil {
		return
	}
	s := outer
	if stmt.Background {
		s = isolated(outer)
	}
	switch cmd := stmt.Cmd.(type) {
	case *syntax.CallExpr:
		f.simple(cmd, stmt.Redirs, s)
		return
	case *syntax.DeclClause:
		f.simple(cmd, stmt.Redirs, s)
		return
	}
	if len(stmt.Redirs) > 0 {
		f.simple(nil, stmt.Redirs, s)
	}
	if stmt.Cmd == nil {
		return
	}
	switch cmd := stmt.Cmd.(type) {
	case *syntax.BinaryCmd:
		f.binary(cmd, s)
	case *syntax.Subshell:
		inner := isolated(s)
		for _, x := range cmd.Stmts {
			f.statement(x, inner)
		}
	default:
		f.children(stmt.Cmd, s)
	}
}

func movedOnSuccess(stmt *syntax.Stmt) bool {
	var state func(*syntax.Stmt) string
	state = func(s *syntax.Stmt) string {
		if s == nil || s.Cmd == nil {
			return "unchanged"
		}
		switch c := s.Cmd.(type) {
		case *syntax.CallExpr:
			name := ""
			if len(c.Args) > 0 && len(c.Args[0].Parts) > 0 {
				if lit, ok := c.Args[0].Parts[0].(*syntax.Lit); ok {
					name = lit.Value
				}
			}
			if name == "popd" {
				return "uncertain"
			}
			if name != "cd" && name != "pushd" {
				return "unchanged"
			}
			if len(c.Args) > 1 && len(c.Args[1].Parts) == 1 {
				if lit, ok := c.Args[1].Parts[0].(*syntax.Lit); ok && lit.Value != "-" {
					return "moved"
				}
			}
			return "uncertain"
		case *syntax.BinaryCmd:
			if c.Op != syntax.AndStmt {
				return "uncertain"
			}
			l, r := state(c.X), state(c.Y)
			if l == "uncertain" || r == "uncertain" {
				return "uncertain"
			}
			if l == "moved" || r == "moved" {
				return "moved"
			}
			return "unchanged"
		}
		return "uncertain"
	}
	return state(stmt) == "moved"
}

func (f *frontend) binary(cmd *syntax.BinaryCmd, s scope) {
	op := cmd.Op.String()
	if op == "&&" && movedOnSuccess(cmd.X) {
		outer := s.dir.failures
		failures := []string{}
		s.dir.failures = &failures
		f.statement(cmd.X, s)
		if _, ok := cmd.Y.Cmd.(*syntax.CallExpr); ok {
			s.dir.failures = &failures
		} else {
			s.dir.failures = nil
		}
		f.statement(cmd.Y, s)
		s.dir.failures = outer
		if outer != nil {
			*outer = append(*outer, failures...)
		} else {
			s.dir.alternatives = boundedDirectories(s.dir.cwd, append(append([]string{}, s.dir.alternatives...), failures...), f.home)
		}
		return
	}
	failed := isolated(s)
	start := len(f.script.Commands)
	leftScope := s
	if op != "&&" && op != "||" {
		leftScope = isolated(s)
	}
	f.statement(cmd.X, leftScope)
	middle := len(f.script.Commands)
	right := isolated(s)
	if op == "||" {
		right = failed
	} else if op == "&&" {
		right = s
	}
	f.statement(cmd.Y, right)
	if op == "|" || op == "|&" {
		left := f.script.Commands[start:middle]
		rest := f.script.Commands[middle:]
		markWalkedInput(left, rest)
		sources := append(xargsReplacements(left, rest), shellInput(left, rest)...)
		for _, item := range sources {
			f.parse(item.source, item.cwd)
		}
	}
	if op != "&&" {
		candidates := append([]string{}, s.dir.alternatives...)
		candidates = append(candidates, right.dir.cwd)
		candidates = append(candidates, right.dir.alternatives...)
		s.dir.alternatives = boundedDirectories(s.dir.cwd, candidates, f.home)
	}
}

func (f *frontend) children(node syntax.Node, s scope) {
	if fn, ok := node.(*syntax.FuncDecl); ok {
		if fn.Name != nil && fn.Body != nil {
			f.functions[fn.Name.Value] = function{fn.Body, s.src}
		}
		s = isolated(s)
	}
	if c, ok := node.(*syntax.IfClause); ok {
		f.conditional(c, s)
		return
	}
	outer := s
	loop := false
	switch node.(type) {
	case *syntax.WhileClause, *syntax.ForClause:
		loop = true
		s = isolated(s)
	}
	syntax.Walk(node, func(child syntax.Node) bool {
		if child == nil || child == node {
			return true
		}
		switch c := child.(type) {
		case *syntax.Stmt:
			f.statement(c, s)
			return false
		case *syntax.Word:
			value := f.word(c, s)
			cwds := append([]string{s.dir.cwd}, s.dir.alternatives...)
			for _, cwd := range cwds {
				f.script.Commands = append(f.script.Commands, &record.Command{Argv: []*record.Word{value}, Redirects: []record.Redirect{}, Cwd: cwd, Program: -1, Wrappers: []string{}, Shell: true})
			}
			return false
		}
		return true
	})
	if loop {
		candidates := append([]string{}, outer.dir.alternatives...)
		candidates = append(candidates, s.dir.cwd)
		candidates = append(candidates, s.dir.alternatives...)
		outer.dir.alternatives = boundedDirectories(outer.dir.cwd, candidates, f.home)
	}
}

func (f *frontend) conditional(node *syntax.IfClause, s scope) {
	condition := isolated(s)
	for _, stmt := range node.Cond {
		f.statement(stmt, condition)
	}
	then := isolated(condition)
	for _, stmt := range node.Then {
		f.statement(stmt, then)
	}
	branches := []scope{s, condition, then}
	if node.Else != nil {
		other := isolated(condition)
		f.conditional(node.Else, other)
		branches = append(branches, other)
	}
	candidates := []string{}
	for _, b := range branches {
		candidates = append(candidates, b.dir.cwd)
		candidates = append(candidates, b.dir.alternatives...)
	}
	s.dir.alternatives = boundedDirectories(s.dir.cwd, candidates, f.home)
}
