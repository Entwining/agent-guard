package targets

import (
	"agentguard/native/record"
	"slices"
	"strings"
)

var gitMetadata = strings.Fields("add rm mv restore checkout reset stash check-ignore check-attr update-index ls-files status clean commit")
var gitNames = strings.Fields("branch tag remote switch push fetch pull merge rebase cherry-pick revert reflog rev-parse describe bisect init clone submodule worktree config lfs sparse-checkout")
var gitFileOptions = func() map[string][]string {
	m := map[string][]string{"config": {"-f", "--file", "--blob"}, "commit": {"-F", "--file", "--pathspec-from-file"}, "tag": {"-F", "--file"}, "merge": {"-F", "--file"}}
	for _, s := range strings.Fields("add rm restore reset checkout stash") {
		m[s] = []string{"--pathspec-from-file"}
	}
	return m
}()
var GitValueOptions = []string{"-C", "-c", "--git-dir", "--work-tree", "--namespace", "--exec-path"}

func gitTargets(c *Context) []record.Target {
	ts := []record.Target{}
	i := 0
	base := ""
	for i < len(c.Words) && strings.HasPrefix(c.Words[i].Text, "-") {
		text := c.Words[i].Text
		glued := strings.HasPrefix(text, "--work-tree=")
		if rx(`^--(namespace|exec-path)=`, text) {
			c.Claimed[c.Words[i]] = true
		}
		takes := !glued && slices.Contains(GitValueOptions, text)
		w := record.At(c.Words, i+1)
		if glued {
			w = c.Words[i]
		}
		if (glued || takes && (text == "-C" || text == "--work-tree")) && w != nil {
			c.Claimed[w] = true
			p := w.Text
			if glued {
				p = strings.TrimPrefix(text, "--work-tree=")
			}
			t := c.Make(p, w, "enter", Options{Via: "option", Base: base})
			ts = append(ts, t)
			base = t.Path
		}
		i++
		if takes {
			i++
		}
	}
	sub := record.Text(c.Words, i)
	if w := record.At(c.Words, i); w != nil {
		c.Claimed[w] = true
	}
	effect := "read"
	if slices.Contains(gitMetadata, sub) {
		effect = "meta"
	} else if slices.Contains(gitNames, sub) {
		effect = "name"
	}
	ops := []*record.Word{}
	if i+1 < len(c.Words) {
		ops = c.Words[i+1:]
	}
	if sub == "grep" {
		rest := []*record.Word{}
		patterned, options := false, true
		for n := 0; n < len(ops); n++ {
			t := ops[n].Text
			if options && t == "--" {
				options = false
			} else if options && t == "-e" {
				patterned = true
				n++
				if w := record.At(ops, n); w != nil {
					c.Claimed[w] = true
				}
			} else if options && t == "-f" {
				patterned = true
				n++
				if w := record.At(ops, n); w != nil {
					rest = append(rest, w)
				}
			} else if options && strings.HasPrefix(t, "-") {
				rest = append(rest, ops[n])
			} else if !patterned {
				patterned = true
				c.Claimed[ops[n]] = true
			} else {
				rest = append(rest, ops[n])
			}
		}
		ops = rest
	}
	keys := gitFileOptions[sub]
	pathspec := effect != "name" && sub != "grep"
	add := func(p string, w *record.Word, e, via string) {
		c.Claimed[w] = true
		glob := pathspec && strings.ContainsAny(p, "*?[")
		o := Options{Via: via, Glob: B(glob), Base: base}
		ts = append(ts, c.Make(p, w, e, o))
		if e == "read" && strings.Contains(p, ":") {
			ts = append(ts, c.Make(p[strings.Index(p, ":")+1:], w, e, o))
		}
	}
	var action, file *record.Word
	if sub == "bundle" {
		for _, w := range ops {
			if !strings.HasPrefix(w.Text, "-") {
				if action == nil {
					action = w
				} else if file == nil && action.Text == "create" {
					file = w
					break
				}
			}
		}
	}
	for n := 0; n < len(ops); n++ {
		w := ops[n]
		glued := ""
		for _, k := range keys {
			if strings.HasPrefix(k, "--") && strings.HasPrefix(w.Text, k+"=") || !strings.HasPrefix(k, "--") && len(w.Text) > len(k) && strings.HasPrefix(w.Text, k) {
				glued = k
				break
			}
		}
		if glued != "" {
			offset := len(glued)
			if strings.HasPrefix(glued, "--") {
				offset++
			}
			add(w.Text[offset:], w, "read", "option")
		} else if slices.Contains(keys, w.Text) && n+1 < len(ops) {
			n++
			add(ops[n].Text, ops[n], "read", "option")
		} else if w == action {
			c.Claimed[w] = true
		} else if !strings.HasPrefix(w.Text, "-") {
			e := effect
			if w == file {
				e = "write"
			}
			add(w.Text, w, e, "operand")
		}
	}
	return ts
}
