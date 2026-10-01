package record

import (
	"encoding/json"
	"path"
	"slices"
)

type Word struct {
	Text    string   `json:"text"`
	Raw     string   `json:"raw"`
	Expands bool     `json:"expands"`
	Globs   bool     `json:"globs"`
	Vars    []string `json:"vars"`
	Role    string   `json:"role"`
	Value   string   `json:"value"`
	Pwd     bool     `json:"pwd"`
}
type Redirect struct {
	Direction string   `json:"direction"`
	Target    string   `json:"target"`
	Globs     bool     `json:"globs"`
	Expands   bool     `json:"expands"`
	Vars      []string `json:"vars"`
}
type Items struct {
	Root   string `json:"root"`
	Hidden bool   `json:"hidden"`
}
type Target struct {
	Path       string `json:"path"`
	Unresolved string `json:"unresolved"`
	Glob       bool   `json:"glob"`
	Effect     string `json:"effect"`
	Walk       string `json:"walk"`
	Sends      bool   `json:"sends"`
	Expands    bool   `json:"expands"`
	Via        string `json:"via"`
	Search     bool   `json:"search"`
	Command    int    `json:"command"`
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

func (f Flags) MarshalJSON() ([]byte, error) {
	if f == nil {
		return []byte("[]"), nil
	}
	return json.Marshal([]string(f))
}

type Command struct {
	Argv      []*Word    `json:"argv"`
	Redirects []Redirect `json:"redirects"`
	Cwd       string     `json:"cwd"`
	Program   int        `json:"program"`
	Wrappers  []string   `json:"wrappers"`
	Shell     bool       `json:"shell"`
	Flags     Flags      `json:"flags"`
	Items     *Items     `json:"items,omitempty"`
}
type Fragment struct {
	Text string `json:"text"`
	Cwd  string `json:"cwd"`
}
type Script struct {
	Commands      []*Command `json:"commands"`
	Uninspectable []Fragment `json:"uninspectable"`
	ParseFailed   bool       `json:"parseFailed"`
}
type Request struct {
	Runtime    string `json:"runtime"`
	Tool       string `json:"tool"`
	Home       string `json:"home"`
	Cwd        string `json:"cwd"`
	InputCwd   string `json:"inputCwd"`
	PathInput  string `json:"pathInput"`
	Operation  string `json:"operation"`
	Target     string `json:"target"`
	SearchRoot string `json:"searchRoot"`
	Glob       string `json:"glob"`
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
