package rules

import (
	text "agentguard/native"
	"agentguard/native/reasons"
	"agentguard/native/record"
	"agentguard/native/targets"
	"regexp"
	"slices"
	"strings"
)

func secretName(s string) bool { return rx(`TOKEN|SECRET|KEY|PASSWORD|CREDENTIAL`, strings.ToUpper(s)) }

func SecretReasons(c *record.Command) []string {
	d := []string{}
	if c.Program < 0 {
		if len(c.Wrappers) > 0 && c.Wrappers[len(c.Wrappers)-1] == "env" && !slices.Contains(c.Wrappers, "env-S") {
			d = append(d, reasons.Dump)
		}
		return d
	}
	program := c.Argv[c.Program].Text
	name := record.ProgramName(program)
	ws := record.Rest(c)
	args := record.Args(c)
	a := func(i int) string {
		if i < len(args) {
			return args[i]
		}
		return ""
	}
	if c.Shell && !strings.Contains(program, "/") {
		if name == "export" {
			if len(args) == 0 {
				d = append(d, reasons.Dump)
			} else {
				for _, s := range args {
					if rx(`^-[^-]*p`, s) {
						d = append(d, reasons.Dump)
						break
					}
				}
			}
		}
		if name == "set" && len(args) == 0 {
			d = append(d, reasons.Dump)
		}
		if name == "typeset" || name == "declare" {
			if len(args) == 0 || len(args) == 1 && rx(`^-[^-]*[px]`, args[0]) {
				d = append(d, reasons.Dump)
			}
			for _, s := range args {
				if !strings.HasPrefix(s, "-") && !strings.Contains(s, "=") && secretName(s) {
					d = append(d, reasons.Variable)
				}
			}
		}
	}
	display := slices.Contains(targets.Readers, name) || slices.Contains([]string{"echo", "printf", "print"}, name)
	switch name {
	case "printenv":
		ops := 0
		for _, s := range args {
			if !strings.HasPrefix(s, "-") {
				ops++
				if secretName(s) {
					d = append(d, reasons.Variable)
				}
			}
		}
		if ops == 0 {
			d = append(d, reasons.Dump)
		}
	case "echo", "printf", "print":
		if name != "echo" && a(0) == "-v" {
			display = false
		}
		if display {
			hit := false
			for _, w := range ws {
				for _, v := range w.Vars {
					hit = hit || secretName(v)
				}
			}
			if hit {
				d = append(d, reasons.Variable)
			}
		}
	case "gh":
		if a(0) == "auth" && (a(1) == "token" || a(1) == "status" && ghShowsToken(args[2:])) {
			d = append(d, reasons.Token)
		}
	case "glab":
		if a(0) == "auth" && a(1) == "status" {
			for _, s := range args {
				if rx(`^(--show-token|-[^-]*t)`, s) {
					d = append(d, reasons.Token)
					break
				}
			}
		}
	case "security":
		hit := a(0) == "dump-keychain" || a(0) == "export"
		for _, s := range args {
			hit = hit || rx(`^-[A-Za-z]*[wg]`, s) && !strings.HasPrefix(s, "--")
		}
		if hit {
			d = append(d, reasons.Keychain)
		}
	case "gcloud":
		if slices.Contains(args, "print-access-token") || slices.Contains(args, "print-identity-token") {
			d = append(d, reasons.SecretPrint)
		}
	case "az":
		if a(0) == "account" && a(1) == "get-access-token" {
			d = append(d, reasons.SecretPrint)
		}
	case "aws":
		if a(0) == "configure" && a(1) == "get" && secretName(a(2)) {
			d = append(d, reasons.SecretPrint)
		}
	case "npm":
		if a(0) == "config" && a(1) == "get" && rx(`(?i)auth|token|password`, a(2)) {
			d = append(d, reasons.SecretPrint)
		}
	case "kubectl":
		if a(0) == "config" && a(1) == "view" && slices.Contains(args, "--raw") {
			d = append(d, reasons.SecretPrint)
		}
	case "gpg":
		for _, s := range args {
			if rx(`^--export-secret-(sub)?keys$`, s) {
				d = append(d, reasons.SecretPrint)
				break
			}
		}
	case "curl":
		if curlTraces(args) {
			d = append(d, reasons.Trace)
		}
	case "git":
		i := 0
		for i < len(args) && strings.HasPrefix(args[i], "-") {
			step := 1
			if slices.Contains(targets.GitValueOptions, args[i]) {
				step = 2
			}
			i += step
		}
		if i+1 < len(args) && args[i] == "credential" && args[i+1] == "fill" {
			d = append(d, reasons.SecretPrint)
		}
	}
	if display {
		hit := false
		for _, r := range c.Redirects {
			for _, v := range r.Vars {
				hit = hit || secretName(v)
			}
		}
		if hit {
			d = append(d, reasons.Variable)
		}
	}
	return d
}

func ghShowsToken(args []string) bool {
	for i := 0; i < len(args); i++ {
		s := args[i]
		if s == "--" {
			return false
		}
		if slices.Contains([]string{"--hostname", "--jq", "--json", "--template", "-h"}, s) {
			i++
		} else if s == "--show-token" || strings.HasPrefix(s, "--show-token=") || rx(`^-[at]*t`, s) {
			return true
		}
	}
	return false
}

func curlTraces(args []string) bool {
	for i := 0; i < len(args); i++ {
		s := args[i]
		if s == "--" {
			break
		}
		if rx(`^--(verbose|trace|trace-ascii)(=|$)`, s) {
			return true
		}
		if rx(`^--(data|data-ascii|data-binary|data-urlencode|json|form|header|upload-file|config)$`, s) {
			i++
		}
		if !rx(`^-[^-]`, s) {
			continue
		}
		for k := 1; k < len(s); k++ {
			if s[k] == 'v' {
				return true
			}
			if !strings.ContainsRune(targets.CurlValueLetters, rune(s[k])) {
				continue
			}
			if k == len(s)-1 {
				i++
			}
			break
		}
	}
	return false
}

func SecretSignatures(fragment string) []string {
	d := []string{}
	fragment = regexp.MustCompile(`&&|\|\||\n`).ReplaceAllString(fragment, ";")
	for _, s := range strings.Split(fragment, ";") {
		if rx(`(^|[^A-Za-z0-9_-])printenv([^A-Za-z0-9_-]|$)`, s) || rx(`^`+text.Space+`*\(*`+text.Space+`*(env|export|set|typeset|declare)`+text.Space+`*($|[|>)])`, s) || rx(`(declare|typeset|export)`+text.Space+`+-[a-z]*[px]`, s) {
			d = append(d, reasons.Dump)
		}
		if rx(`(?s)curl`+text.Space+`.*`+text.Space+`(-[A-Za-z]*v[A-Za-z]*|--verbose|--trace(-ascii)?)(`+text.Space+`|=|$)`, s) {
			d = append(d, reasons.Trace)
		}
		if rx(`gh`+text.Space+`+auth`+text.Space+`+token`, s) {
			d = append(d, reasons.Token)
		}
		if rx(`(?s)security`+text.Space+`.*`+text.Space+`-[wg](`+text.Space+`|$)`, s) {
			d = append(d, reasons.Keychain)
		}
		if rx(`(?s)(ECHO|PRINTF|PRINT)`+text.Space+`.*\$\{?[A-Z0-9_]*(TOKEN|SECRET|KEY|PASSWORD|CREDENTIAL)`, strings.ToUpper(s)) {
			d = append(d, reasons.Variable)
		}
	}
	return d
}
