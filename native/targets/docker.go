package targets

import (
	"agentguard/native/record"
	"slices"
	"strings"
)

var dockerGlobal = strings.Fields("-H --host -c --context -l --log-level --config --tlscacert --tlscert --tlskey")
var dockerFlags = strings.Fields("--detach --help --init --interactive --no-healthcheck --oom-kill-disable --privileged --publish-all --quiet --read-only --rm --sig-proxy --tty --use-api-socket")
var composeValues = strings.Fields("-f --file -p --project-name --project-directory --profile --env-file --ansi --parallel --progress")

func dockerCommandStart(ws []*record.Word, start int) int {
	for i := start + 1; i < len(ws); i++ {
		text := ws[i].Text
		if !strings.HasPrefix(text, "-") {
			return i
		}
		if text == "--" {
			return i + 1
		}
		if strings.HasPrefix(text, "--") {
			if !strings.Contains(text, "=") && (text == "--entrypoint" || !slices.Contains(dockerFlags, text) && !rx(`^-\D`, record.Text(ws, i+1))) {
				i++
			}
			continue
		}
		for k := 1; k < len(text); k++ {
			if !strings.ContainsRune("diqPt", rune(text[k])) {
				if k == len(text)-1 {
					i++
				}
				break
			}
		}
	}
	return len(ws)
}

func dockerTargets(c *Context) []record.Target {
	ws := c.Words
	ts := []record.Target{}
	mount := func(p string, w *record.Word, e, via string) {
		c.Claimed[w] = true
		ts = append(ts, c.Make(p, w, e, Options{Via: via}))
	}
	start := 0
	for strings.HasPrefix(record.Text(ws, start), "-") {
		if slices.Contains(dockerGlobal, ws[start].Text) {
			start += 2
		} else {
			start++
		}
	}
	if slices.Contains([]string{"container", "image", "buildx"}, record.Text(ws, start)) {
		start++
	}
	sub := record.Text(ws, start)
	end := len(ws)
	if slices.Contains([]string{"run", "create", "exec"}, sub) {
		end = dockerCommandStart(ws, start)
	}
	composeSub := start + 1
	for strings.HasPrefix(record.Text(ws, composeSub), "-") {
		if slices.Contains(composeValues, ws[composeSub].Text) {
			composeSub += 2
		} else {
			composeSub++
		}
	}
	for i, w := range ws[:end] {
		prev := record.Text(ws, i-1)
		if prev == "--entrypoint" {
			continue
		}
		if sub == "cp" && i > start && !strings.HasPrefix(w.Text, "-") && !rx(`^[\w.-]+:`, w.Text) {
			e := "read"
			if i == len(ws)-1 {
				e = "write"
			}
			mount(w.Text, w, e, "operand")
		}
		if sub == "load" {
			input := ""
			if m := match(`(?s)^(?:--input=|-i)(.+)$`, w.Text); m != nil {
				input = m[1]
			} else if prev == "-i" || prev == "--input" {
				input = w.Text
			}
			if input != "" {
				mount(input, w, "read", "option")
			}
		}
		secret := ""
		if strings.HasPrefix(w.Text, "--secret=") {
			secret = strings.TrimPrefix(w.Text, "--secret=")
		} else if prev == "--secret" {
			secret = w.Text
		}
		for _, f := range strings.Split(secret, ",") {
			if rx(`^(src|source)=`, f) {
				mount(f[strings.Index(f, "=")+1:], w, "read", "option")
			}
		}
		host := match(`(?s)^--(file|label-file|cidfile|iidfile|tlscacert|tlscert|tlskey|config)(?:=(.*))?$`, w.Text)
		if host == nil && (sub == "build" || sub == "compose" && i < composeSub) {
			host = match(`(?s)^-[A-Za-z]*?(f)=?(.*)$`, w.Text)
		}
		if host != nil {
			holder := w
			p := host[2]
			if p == "" {
				holder = record.At(ws, i+1)
				if holder != nil {
					p = holder.Text
				}
			}
			if holder != nil {
				e := "use"
				if host[1] == "cidfile" || host[1] == "iidfile" {
					e = "write"
				}
				mount(p, holder, e, "option")
			}
		}
		long := match(`(?s)^--(volume|mount)(?:=(.*))?$`, w.Text)
		short := match(`(?s)^-[A-Za-z]*?v(=?)(.*)$`, w.Text)
		key, inline := "", ""
		if long != nil {
			key = long[1]
			inline = long[2]
		} else if short != nil {
			key = "volume"
			inline = short[2]
		}
		if key == "" {
			continue
		}
		holder := w
		spec := inline
		if spec == "" {
			holder = record.At(ws, i+1)
			if holder == nil {
				continue
			}
			spec = holder.Text
		}
		if key == "volume" {
			mount(strings.Split(spec, ":")[0], holder, "read", "option")
		} else {
			for _, f := range strings.Split(spec, ",") {
				if rx(`^(src|source)=`, f) {
					mount(f[strings.Index(f, "=")+1:], holder, "read", "option")
				}
			}
		}
	}
	return ts
}
