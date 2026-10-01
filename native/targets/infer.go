package targets

import (
	text "agentguard/native"
	"agentguard/native/filesystem"
	"agentguard/native/record"
	"path"
	"regexp"
	"slices"
	"strings"
)

func maker(home string, cmd *record.Command, command int, walk string, sends bool) func(string, *record.Word, string, Options) record.Target {
	return func(p string, w *record.Word, effect string, o Options) record.Target {
		quoted := w != nil && rx(`^['"]`, w.Raw)
		if o.Quoted != nil {
			quoted = *o.Quoted
		}
		input := p
		if !quoted {
			input = filesystem.ExpandHome(input, home)
		}
		input = filesystem.StripFileURL(input)
		base := cmd.Cwd
		if o.Base != "" {
			base = o.Base
		}
		unresolved := input
		if !strings.HasPrefix(input, "/") {
			unresolved = base + "/" + input
		}
		t := record.Target{Path: filesystem.AbsPath(p, base, home, quoted), Unresolved: unresolved, Effect: effect, Walk: walk, Sends: sends, Via: "operand", Command: command}
		if w != nil {
			t.Glob = w.Globs
			t.Expands = w.Expands
		}
		if o.Via != "" {
			t.Via = o.Via
		}
		if o.Walk != "" {
			t.Walk = o.Walk
		}
		if o.Glob != nil {
			t.Glob = *o.Glob
		}
		if o.Expands != nil {
			t.Expands = *o.Expands
		}
		if o.Sends != nil {
			t.Sends = *o.Sends
		}
		if o.Search != nil {
			t.Search = *o.Search
		}
		return t
	}
}

func operandValue(w *record.Word) string {
	v := w.Value
	if strings.HasPrefix(v, "-") {
		i := strings.Index(v, "=")
		if i < 0 {
			return ""
		}
		v = v[i+1:]
	}
	if strings.HasPrefix(v, "@") {
		return v[1:]
	}
	if m := match(`(?s)^[^=@`+text.SpaceClass+`]+?(?:==?|:)@(.+)$`, v); m != nil {
		return m[1]
	}
	return v
}

func commandTargets(cmd *record.Command, index int, home string) []record.Target {
	program := record.At(cmd.Argv, cmd.Program)
	name := ""
	if program != nil {
		name = record.ProgramName(program.Text)
	}
	spec, modelled := Specs[name]
	ws := record.Rest(cmd)
	walk := walkOf(spec, name, ws)
	options := map[string]string{}
	for k, v := range GlobalOptions {
		options[k] = v
	}
	for k, v := range spec.Options {
		options[k] = v
	}
	optionEffect := func(i int) string {
		w := ws[i]
		prev := record.Text(ws, i-1)
		key := prev
		if strings.HasPrefix(w.Value, "-") {
			key = ""
			if at := strings.Index(w.Value, "="); at >= 0 {
				key = w.Value[:at]
			}
		} else if rx(`^-[^-]`, prev) {
			key = "-" + prev[len(prev)-1:]
		}
		return options[key]
	}
	ops := []*record.Word{}
	for i, w := range ws {
		if !strings.HasPrefix(w.Text, "-") && optionEffect(i) == "" {
			ops = append(ops, w)
		}
	}
	last := record.At(ops, len(ops)-1)
	into := false
	if spec.Options["-t"] == "write" {
		for _, w := range ws {
			into = into || rx(`^-[^-]*t`, w.Text) || rx(`^--t[a-z-]*(=|$)`, w.Text) && strings.HasPrefix("--target-directory", strings.Split(w.Text, "=")[0])
		}
	}
	var dest *record.Word
	if spec.Last != "" && (last == nil || !last.Globs) && !into {
		dest = last
	}
	remote := func(w *record.Word) bool { return spec.Remote != nil && spec.Remote.MatchString(w.Value) }
	sends := spec.Sends && spec.Remote == nil
	if spec.Sends && spec.Remote != nil {
		for _, w := range ops {
			sends = sends || remote(w)
		}
	}
	make := maker(home, cmd, index, walk, sends)
	c := &Context{Cmd: cmd, Words: ws, Walk: walk, Claimed: map[*record.Word]bool{}, Make: make}
	operands := spec.Operands
	if operands == "" {
		operands = "read"
		if program == nil {
			operands = "use"
		}
	}
	ts := []record.Target{}
	for _, r := range cmd.Redirects {
		if (r.Direction == "in" || r.Direction == "out") && r.Target != "" {
			effect := "read"
			if r.Direction == "out" {
				effect = "write"
			}
			ts = append(ts, make(r.Target, nil, effect, Options{Via: "redirect", Glob: B(r.Globs), Expands: B(r.Expands)}))
		}
	}
	if cmd.Items != nil && program != nil {
		walk := "visible"
		if cmd.Items.Hidden {
			walk = "hidden"
		}
		ts = append(ts, make(cmd.Items.Root, nil, operands, Options{Via: "items", Glob: B(false), Walk: walk}))
	}
	if slices.Contains(cmd.Wrappers, "xargs") && program != nil {
		os := cmd.Argv[:cmd.Program]
		for i, w := range os {
			if (w.Text == "-a" || w.Text == "--arg-file") && i+1 < len(os) {
				f := os[i+1]
				ts = append(ts, make(f.Text, f, "read", Options{Via: "option"}))
			} else if strings.HasPrefix(w.Text, "--arg-file=") {
				ts = append(ts, make(strings.TrimPrefix(w.Text, "--arg-file="), w, "read", Options{Via: "option"}))
			}
		}
	}
	if program != nil && strings.Contains(program.Value, "/") {
		ts = append(ts, make(program.Value, program, "use", Options{Via: "option"}))
	}
	start := len(ts)
	if spec.Targets != nil {
		ts = append(ts, spec.Targets(c)...)
	}
	for i, w := range ws {
		if !c.Claimed[w] && rx(`^-[^-]`, w.Text) {
			at := -1
			for k := 1; k < len(w.Text); k++ {
				if options["-"+w.Text[k:k+1]] != "" {
					at = k
					break
				}
			}
			if at > 0 && at < len(w.Text)-1 {
				ts = append(ts, make(w.Text[at+1:], w, options["-"+w.Text[at:at+1]], Options{Via: "option"}))
				continue
			}
		}
		if c.Claimed[w] || !slices.Contains([]string{"arg", "path", "patfile", "option:patfile", "optarg"}, w.Role) {
			continue
		}
		v := operandValue(w)
		if v == "" {
			continue
		}
		effect := operands
		via := "operand"
		if w.Role == "optarg" {
			effect = "use"
			via = "option"
		}
		if e := optionEffect(i); e != "" {
			effect = e
		}
		if w == dest {
			effect = spec.Last
		}
		if remote(w) {
			effect = "name"
		}
		ts = append(ts, make(v, w, effect, Options{Via: via}))
	}
	lists := spec.Cwd != ""
	for _, t := range ts[start:] {
		if t.Via == "operand" {
			lists = false
		}
	}
	if lists {
		ts = append(ts, make(cmd.Cwd, nil, "list", Options{Via: spec.Cwd}))
	}
	named := false
	for _, t := range ts {
		named = named || slices.Contains([]string{"operand", "cwd", "scan"}, t.Via) && !slices.Contains([]string{"enter", "name"}, t.Effect)
	}
	if program != nil && !slices.Contains(DataPrograms, name) && !slices.Contains([]string{"cd", "pushd", "popd"}, name) && (!named || !modelled) {
		ts = append(ts, make(cmd.Cwd, nil, "enter", Options{Via: "cwd", Walk: "none"}))
	}
	return ts
}

func codeTokens(code string) []string {
	rxWords := regexp.MustCompile(`[\w.~/-]+`)
	r := []string{}
	for _, at := range rxWords.FindAllStringIndex(code, -1) {
		v := code[at[0]:at[1]]
		before, after := byte(0), byte(0)
		if at[0] > 0 {
			before = code[at[0]-1]
		}
		if at[1] < len(code) {
			after = code[at[1]]
		}
		if strings.HasPrefix(v, ".") || strings.HasPrefix(v, "~") || strings.Contains(v, "/") || strings.ContainsRune("'\"`", rune(before)) || strings.ContainsRune("'\"`", rune(after)) {
			r = append(r, v)
		}
	}
	for i := 0; i < len(code); i++ {
		quote := code[i]
		if !strings.ContainsRune("'\"`", rune(quote)) {
			continue
		}
		j := i + 1
		for j < len(code) && code[j] != quote && code[j] != '\n' {
			j++
		}
		if j < len(code) && code[j] == quote {
			r = append(r, rxWords.FindAllString(code[i+1:j], -1)...)
			i = j
		}
	}
	return r
}

func ExtractTargets(req record.Request) []record.Target {
	ts := []record.Target{}
	add := func(p, cwd, inputCwd, effect string, glob, search bool, walk, via string) {
		input := filesystem.ExpandHome(p, req.Home)
		u := input
		if !strings.HasPrefix(input, "/") {
			u = inputCwd + "/" + input
		}
		ts = append(ts, record.Target{Path: filesystem.AbsPath(p, cwd, req.Home), Unresolved: u, Effect: effect, Walk: walk, Glob: glob, Search: search, Via: via, Command: -1})
	}
	if req.Operation == "read" || req.Operation == "write" {
		add(req.PathInput, req.Cwd, req.InputCwd, req.Operation, false, false, "none", "tool")
	}
	if req.Operation == "search" {
		p := req.PathInput
		if p == "" {
			p = req.InputCwd
		}
		add(p, req.Cwd, req.InputCwd, "read", false, true, "visible", "tool")
		if req.Glob != "" && !strings.HasPrefix(req.Glob, "!") {
			add(req.SearchRoot+"/"+path.Base(req.Glob), req.Cwd, req.InputCwd, "read", true, false, "none", "tool")
		}
	}
	for i, c := range req.Commands {
		ts = append(ts, commandTargets(c, i, req.Home)...)
	}
	for _, f := range req.Uninspectable {
		for _, t := range codeTokens(f.Text) {
			add(t, f.Cwd, f.Cwd, "read", false, false, "none", "code")
		}
	}
	return ts
}
