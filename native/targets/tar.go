package targets

import (
	"agentguard/native/record"
	"strings"
)

func tarCluster(w *record.Word, i int) bool {
	return !strings.HasPrefix(w.Text, "--") && (strings.HasPrefix(w.Text, "-") || i == 0)
}

func tarTargets(c *Context) []record.Target {
	ts := []record.Target{}
	base := ""
	creates, extracts, stdout := false, false, false
	var archive *record.Word
	archivePath := ""
	for i, w := range c.Words {
		cluster := tarCluster(w, i)
		creates = creates || w.Text == "--create" || cluster && rx(`^-?[^CfT]*c`, w.Text)
		extracts = extracts || rx(`^--(extract|get)$`, w.Text) || cluster && rx(`^-?[^CfT]*x`, w.Text)
		stdout = stdout || w.Text == "--to-stdout" || cluster && rx(`^-?[^CfT]*O`, w.Text)
		if archive != nil {
			continue
		}
		switch {
		case strings.HasPrefix(w.Text, "--file="):
			archive = w
			archivePath = strings.TrimPrefix(w.Text, "--file=")
		case w.Text == "--file" && i+1 < len(c.Words):
			archive = c.Words[i+1]
			archivePath = archive.Text
		case cluster:
			m := match(`(?s)^-?[^Cf]*f(.*)$`, w.Text)
			if m != nil {
				if m[1] != "" {
					archive = w
					archivePath = m[1]
				} else if i+1 < len(c.Words) {
					archive = c.Words[i+1]
					archivePath = archive.Text
				}
			}
		}
	}
	if archive != nil {
		c.Claimed[archive] = true
		e := "read"
		if creates {
			e = "write"
		}
		ts = append(ts, c.Make(archivePath, archive, e, Options{Via: "option"}))
	}
	for i, w := range c.Words {
		if w == archive {
			continue
		}
		if strings.HasPrefix(w.Text, "--exclude=") {
			c.Claimed[w] = true
			continue
		}
		glued := match(`(?s)^(?:--directory=|-[^-]*?C)(.+)$`, w.Text)
		enters := glued != nil || rx(`^(--directory|--cd|-[^-]*C)$`, record.Text(c.Words, i-1))
		if !enters {
			if base != "" && !strings.HasPrefix(w.Text, "-") {
				c.Claimed[w] = true
				ts = append(ts, c.Make(w.Text, w, "read", Options{Via: "operand", Base: base}))
			}
			continue
		}
		c.Claimed[w] = true
		dir := w.Text
		if glued != nil {
			dir = glued[1]
		}
		e := "enter"
		if extracts {
			e = "write"
		}
		t := c.Make(dir, w, e, Options{Via: "option", Base: base})
		ts = append(ts, t)
		base = t.Path
	}
	if extracts && base == "" && !stdout {
		ts = append(ts, c.Make(".", nil, "write", Options{Via: "option", Walk: "none"}))
	}
	return ts
}
