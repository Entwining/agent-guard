package rules

import (
	"agentguard/native/reasons"
	"agentguard/native/record"
	"path"
	"slices"
)

func Workflow(req record.Request) []string {
	a := []string{}
	for _, c := range req.Commands {
		if c.Program < 0 || path.Base(c.Argv[c.Program].Text) != "rg" {
			continue
		}
		if c.Flags.Has("replace") {
			a = append(a, reasons.Replace)
		}
		if c.Flags.Has("include") {
			a = append(a, reasons.Include)
		}
		if c.Flags.Has("fixed") {
			continue
		}
		for _, w := range record.Rest(c) {
			if (w.Role == "pattern" || w.Role == "option:pattern") && rx(`(^|[^\\])\\\|`, w.Value) {
				a = append(a, reasons.Bre)
			}
		}
	}
	out := []string{}
	for _, item := range a {
		if !slices.Contains(out, item) {
			out = append(out, item)
		}
	}
	return out
}
