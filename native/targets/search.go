package targets

import (
	"path"

	"agentguard/native/record"
)

func searchTargets(name string, c *Context) []record.Target {
	help, files := c.Cmd.Flags.Has("help"), c.Cmd.Flags.Has("files")
	recursive := name == "grep" && c.Cmd.Flags.Has("recursive")
	walk := "visible"
	if recursive || (name == "rg" || name == "ag") && c.Cmd.Flags.Has("hidden") {
		walk = "hidden"
	}
	effect := "read"
	if help || files && walk != "hidden" {
		effect = "list"
	}
	ts := []record.Target{}
	scoped := false
	for i, w := range c.Words {
		var owner *record.Word
		if w.Role == "optarg" {
			owner = record.At(c.Words, i-1)
		} else if w.Role == "option:optarg" {
			owner = w
		}
		switch {
		case w.Role == "path":
			ts = append(ts, c.Make(w.Value, w, effect, Options{Via: "operand", Walk: walk}))
			scoped = true
		case w.Role == "patfile" || w.Role == "option:patfile":
			ts = append(ts, c.Make(w.Value, w, effect, Options{Via: "option", Walk: "none"}))
		case owner != nil && rx(`^--(ignore-file|exclude-from)(=|$)`, owner.Text):
			ts = append(ts, c.Make(w.Value, w, "read", Options{Via: "option", Walk: "none"}))
		default:
			continue
		}
		c.Claimed[w] = true
	}
	if !scoped && (name != "grep" || recursive) {
		via := "scan"
		if name == "grep" {
			via = "cwd"
		}
		ts = append(ts, c.Make(c.Cmd.Cwd, nil, effect, Options{Via: via, Walk: walk, Search: new(!help)}))
	}
	for _, w := range c.Words {
		if w.Role == "glob" || w.Role == "option:glob" {
			c.Claimed[w] = true
			ts = append(ts, c.Make(path.Base(w.Value), w, effect, Options{Via: "option", Glob: new(true), Walk: "none"}))
		}
	}
	return ts
}
