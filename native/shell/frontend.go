package shell

import (
	"maps"
	"strings"

	"mvdan.cc/sh/v3/syntax"

	"agentguard/native/record"
)

type directory struct {
	cwd          string
	alternatives []string
	failures     *[]string
}
type scope struct {
	dir  *directory
	vars map[string]string
	src  string
}
type function struct {
	body *syntax.Stmt
	src  string
}
type frontend struct {
	home         string
	script       record.Script
	functions    map[string]function
	running      map[string]bool
	functionRuns int
}

func ParseScript(source, cwd, home string) record.Script {
	f := &frontend{home: home, script: record.Script{Commands: []*record.Command{}, Uninspectable: []record.Fragment{}}, functions: map[string]function{}, running: map[string]bool{}}
	ss := []string{}
	for _, s := range strings.Split(cwd, "/") {
		if s != "." {
			ss = append(ss, s)
		}
	}
	cwd = strings.Join(ss, "/")
	if cwd == "" {
		cwd = "/"
	}
	f.parse(source, cwd)
	return f.script
}

func (f *frontend) parse(src, cwd string) {
	p := syntax.NewParser(syntax.KeepComments(true), syntax.Variant(syntax.LangBash))
	file, e := p.Parse(strings.NewReader(src), "")
	if e != nil || misreadComment(file, src) {
		f.script.ParseFailed = true
		return
	}
	s := scope{dir: &directory{cwd: cwd}, vars: map[string]string{}, src: src}
	for _, stmt := range file.Stmts {
		f.statement(stmt, s)
	}
}

func slice(s string, start, end uint) string {
	if start > uint(len(s)) {
		return ""
	}
	if end > uint(len(s)) {
		end = uint(len(s))
	}
	return s[start:end]
}

func text(n syntax.Node, s scope) string { return slice(s.src, n.Pos().Offset(), n.End().Offset()) }

func isolated(s scope) scope {
	d := *s.dir
	d.failures = nil
	d.alternatives = append([]string{}, d.alternatives...)
	v := map[string]string{}
	maps.Copy(v, s.vars)
	return scope{&d, v, s.src}
}

func misreadComment(file *syntax.File, src string) bool {
	bad := false
	syntax.Walk(file, func(n syntax.Node) bool {
		if c, ok := n.(*syntax.Comment); ok {
			at := int(c.Hash.Offset())
			if at > 0 && !strings.ContainsRune(" \t\n;&|()<>", rune(src[at-1])) {
				bad = true
			}
		}
		return !bad
	})
	return bad
}
