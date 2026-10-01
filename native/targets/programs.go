package targets

import (
	"agentguard/native/record"
	"regexp"
	"slices"
	"strings"
)

var Readers = strings.Fields("cat head tail less more bat sed awk jq yq base64 xxd od strings diff openssl plutil cp tee tar source . sort uniq cut nl fold rev paste comm join iconv hexdump hd zcat gzcat bzcat xzcat ag ack tac column pr vim vi nvim view perl ruby dd scp rsync zip ed ex hg svn sh bash zsh dash ksh wget php zgrep zless zmore")
var DataPrograms = strings.Fields("echo printf print : true false export set unset typeset declare local")

type Options struct {
	Via, Base, Walk                      string
	Glob, Expands, Sends, Search, Quoted *bool
}

func B(b bool) *bool { return &b }

type Context struct {
	Cmd     *record.Command
	Words   []*record.Word
	Walk    string
	Claimed map[*record.Word]bool
	Make    func(string, *record.Word, string, Options) record.Target
}
type Spec struct {
	Operands, Walk, Recursive, Last, Cwd string
	Options                              map[string]string
	Sends                                bool
	Remote                               *regexp.Regexp
	Targets                              func(*Context) []record.Target
}

var GlobalOptions = map[string]string{"--exclude": "name", "--exclude-dir": "name", "--include": "name"}
var Specs = func() map[string]Spec {
	m := map[string]Spec{}
	for _, n := range Readers {
		m[n] = Spec{}
	}
	for _, n := range DataPrograms {
		m[n] = Spec{Operands: "name", Walk: "none"}
	}
	for _, n := range strings.Fields("stat test [ chmod chown chgrp chflags touch rm rmdir mkdir mv ln wc file shasum sha1sum sha256sum md5 md5sum cksum realpath readlink basename dirname") {
		s := Spec{Operands: "meta"}
		if slices.Contains(strings.Fields("stat test [ mkdir mv"), n) {
			s.Walk = "none"
		}
		m[n] = s
	}
	m["cd"] = Spec{Operands: "name", Walk: "none"}
	for _, n := range []string{"pushd", "popd"} {
		m[n] = Spec{Operands: "enter", Walk: "none"}
	}
	m["jq"] = Spec{Targets: filterTargets(nil)}
	m["yq"] = Spec{Targets: filterTargets([]string{"eval", "e", "eval-all", "ea"})}
	m["gh"] = Spec{}
	m["ls"] = Spec{Operands: "list", Recursive: `^(--recursive$|-[^-]*R)`, Cwd: "cwd"}
	m["tree"] = Spec{Operands: "list", Cwd: "scan"}
	m["du"] = m["tree"]
	into := map[string]string{"-t": "write", "--target-directory": "write"}
	m["cp"] = Spec{Options: into, Last: "write", Recursive: `^(--recursive$|-[^-]*[rR])`}
	m["dd"] = Spec{Walk: "none", Targets: ddTargets}
	m["tar"] = Spec{Targets: tarTargets}
	m["tee"] = Spec{Operands: "write"}
	m["install"] = Spec{Options: into, Last: "write"}
	remote := regexp.MustCompile(`^(rsync://|([^/@:]+@)?[^/@:]+:)`)
	m["scp"] = Spec{Last: "write", Sends: true, Remote: remote, Targets: sshTargets("scp")}
	m["sftp"] = Spec{Targets: sshTargets("sftp")}
	m["rsync"] = Spec{Options: map[string]string{"--files-from": "read", "--exclude-from": "read", "--include-from": "read"}, Last: "write", Sends: true, Remote: remote}
	used := func(s string) map[string]string {
		r := map[string]string{}
		for _, k := range strings.Fields(s) {
			r[k] = "use"
		}
		return r
	}
	m["wget"] = Spec{Operands: "name", Options: used("--ca-certificate --ca-directory --certificate --private-key --crl-file --random-file"), Targets: wgetTargets}
	m["curl"] = Spec{Operands: "name", Options: used("--cacert --capath --cert --key -E --netrc-file --crlfile --egd-file --knownhosts --proxy-cacert --proxy-capath --proxy-cert --proxy-crlfile --proxy-key --random-file --pubkey --pinnedpubkey --proxy-pinnedpubkey --unix-socket"), Targets: curlTargets}
	m["git"] = Spec{Targets: gitTargets}
	m["docker"] = Spec{Operands: "name", Options: used("--env-file"), Targets: dockerTargets}
	m["node"] = Spec{Options: used("--env-file --env-file-if-exists")}
	for _, n := range []string{"bun", "deno"} {
		m[n] = Spec{Options: used("--env-file")}
	}
	m["kubectl"] = Spec{Options: used("--kubeconfig")}
	m["ssh"] = Spec{Operands: "name", Targets: sshTargets("ssh")}
	m["ssh-add"] = Spec{Operands: "use"}
	m["ssh-keygen"] = Spec{Options: used("-f")}
	m["dotenvx"] = Spec{Options: used("-f --file --env-file")}
	for _, n := range []string{"npm", "pnpm", "yarn"} {
		m[n] = Spec{Options: used("--userconfig")}
	}
	for _, n := range []string{"rg", "grep", "ag", "ack"} {
		m[n] = Spec{Targets: func(ctx *Context) []record.Target { return searchTargets(n, ctx) }}
	}
	m["find"] = Spec{Operands: "list", Cwd: "scan", Targets: findTargets}
	m["fd"] = Spec{Operands: "list", Cwd: "scan"}
	return m
}()

func rx(p, s string) bool { return regexp.MustCompile(p).MatchString(s) }

func match(p, s string) []string { return regexp.MustCompile(p).FindStringSubmatch(s) }

func walkOf(s Spec, n string, ws []*record.Word) string {
	if s.Recursive != "" {
		for _, w := range ws {
			if rx(s.Recursive, w.Text) {
				return "visible"
			}
		}
		return "none"
	}
	if n == "git" {
		for _, w := range ws {
			if w.Text == "config" {
				return "none"
			}
		}
	}
	if s.Walk != "" {
		return s.Walk
	}
	return "visible"
}

func FindRoots(ws []*record.Word) []*record.Word {
	r := []*record.Word{}
	i := 0
	for i < len(ws) {
		if ws[i].Text == "-f" && i+1 < len(ws) {
			r = append(r, ws[i+1])
			i += 2
		} else if ws[i].Text == "--" || rx(`^-[HLPEXxdsO]`, ws[i].Text) {
			i++
		} else {
			break
		}
	}
	for ; i < len(ws) && !rx(`^(-|\(|!)`, ws[i].Text); i++ {
		r = append(r, ws[i])
	}
	return r
}

func findTargets(c *Context) []record.Target {
	r := []record.Target{}
	for _, w := range FindRoots(c.Words) {
		c.Claimed[w] = true
		r = append(r, c.Make(w.Text, w, "list", Options{Via: "operand", Walk: "hidden"}))
	}
	for _, w := range c.Words {
		if !slices.Contains([]string{"-exec", "-execdir", "-ok", "-okdir"}, w.Text) {
			c.Claimed[w] = true
		}
	}
	return r
}

func ddTargets(c *Context) []record.Target {
	r := []record.Target{}
	for _, w := range c.Words {
		c.Claimed[w] = true
		if strings.HasPrefix(w.Text, "if=") {
			r = append(r, c.Make(w.Text[3:], w, "read", Options{Via: "option"}))
		} else if strings.HasPrefix(w.Text, "of=") {
			r = append(r, c.Make(w.Text[3:], w, "write", Options{Via: "option"}))
		}
	}
	return r
}

func filterTargets(commands []string) func(*Context) []record.Target {
	return func(c *Context) []record.Target {
		ops := []*record.Word{}
		for _, w := range c.Words {
			if w.Text == "-f" || w.Text == "--from-file" {
				return nil
			}
			if !strings.HasPrefix(w.Text, "-") {
				ops = append(ops, w)
			}
		}
		i := 0
		if slices.Contains(commands, record.Text(ops, 0)) {
			i = 1
		}
		if w := record.At(ops, i); w != nil {
			c.Claimed[w] = true
		}
		return nil
	}
}
