package shell

import (
	"strings"

	"mvdan.cc/sh/v3/syntax"

	"agentguard/native/record"
)

func (f *frontend) redirect(node *syntax.Redirect, s scope) *record.Redirect {
	op := node.Op.String()
	if op == "<<" || op == "<<-" {
		quoted := false
		for _, p := range node.Word.Parts {
			if l, ok := p.(*syntax.Lit); !ok || strings.Contains(l.Value, `\`) {
				quoted = true
			}
		}
		names := []string{}
		body := ""
		if node.Hdoc != nil {
			body = text(node.Hdoc, s)
			if !quoted {
				f.expansions(node.Hdoc, s, &names)
			}
		}
		return &record.Redirect{Direction: "heredoc", Target: body, Vars: names}
	}
	target := f.word(node.Word, s)
	if op == "<<<" {
		return &record.Redirect{Direction: "herestring", Target: target.Text, Vars: target.Vars}
	}
	if (op == "<&" || op == ">&") && rx(`^(\d+|-)$`, target.Text) {
		return nil
	}
	direction := "out"
	if op == "<" || op == "<>" {
		direction = "in"
	}
	return &record.Redirect{Direction: direction, Target: target.Text, Globs: target.Globs, Expands: target.Expands, Vars: []string{}}
}
