package record

import (
	"path"
	"slices"
)

type Word struct {
	Text    string
	Raw     string
	Expands bool
	Globs   bool
	Vars    []string
	Role    string
	Value   string
	Pwd     bool
}
type Redirect struct {
	Direction string
	Target    string
	Globs     bool
	Expands   bool
	Vars      []string
}
type Items struct {
	Root   string
	Hidden bool
}
type Target struct {
	Path       string
	Unresolved string
	Glob       bool
	Effect     string
	Walk       string
	Sends      bool
	Expands    bool
	Via        string
	Search     bool
	Command    int
}
type Flags []string

func (f *Flags) Add(s string) {
	if !f.Has(s) {
		*f = append(*f, s)
	}
}

func (f Flags) Has(s string) bool { return slices.Contains(f, s) }

func (f *Flags) Delete(s string) {
	for i, v := range *f {
		if v == s {
			*f = append((*f)[:i], (*f)[i+1:]...)
			return
		}
	}
}

type Command struct {
	Argv      []*Word
	Redirects []Redirect
	Cwd       string
	Program   int
	Wrappers  []string
	Shell     bool
	Flags     Flags
	Items     *Items
}
type Fragment struct {
	Text string
	Cwd  string
}
type Script struct {
	Commands      []*Command
	Uninspectable []Fragment
	ParseFailed   bool
}
type Request struct {
	Runtime    string
	Tool       string
	Home       string
	Cwd        string
	InputCwd   string
	PathInput  string
	Operation  string
	SearchRoot string
	Glob       string
	Script
}

func ProgramName(s string) string {
	n := path.Base(s)
	if n == "egrep" || n == "fgrep" {
		return "grep"
	}
	return n
}

func Literal(s string) *Word { return &Word{Text: s, Raw: s, Value: s, Role: "arg", Vars: []string{}} }

func At(ws []*Word, i int) *Word {
	if i < 0 || i >= len(ws) {
		return nil
	}
	return ws[i]
}

func Text(ws []*Word, i int) string {
	if w := At(ws, i); w != nil {
		return w.Text
	}
	return ""
}

func Rest(c *Command) []*Word { return c.Argv[c.Program+1:] }

func Args(c *Command) []string {
	r := []string{}
	for _, w := range Rest(c) {
		r = append(r, w.Text)
	}
	return r
}
