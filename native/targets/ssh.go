package targets

import (
	"regexp"
	"strings"

	text "agentguard/native"
	"agentguard/native/record"
)

var sshFileOptions = map[string]string{"identityfile": "use", "certificatefile": "use", "globalknownhostsfile": "use", "userknownhostsfile": "write", "revokedhostkeys": "use", "pkcs11provider": "use"}
var sshValueLetters = map[string]string{"ssh": "BDEFIJLOPQRSWbceilmopw", "scp": "DFJPSXcilo", "sftp": "BDFJPRSXbcilos"}

func sshTargets(client string) func(*Context) []record.Target {
	return func(c *Context) []record.Target {
		ts := []record.Target{}
		operands := 0
		for i := 0; i < len(c.Words); i++ {
			w := c.Words[i]
			if w.Text == "--" {
				break
			}
			if !rx(`^-.`, w.Text) {
				operands++
				stop := 1
				if client == "ssh" {
					stop = 2
				}
				if operands == stop {
					break
				}
				continue
			}
			at := -1
			for k := 1; k < len(w.Text); k++ {
				if strings.ContainsRune(sshValueLetters[client], rune(w.Text[k])) {
					at = k
					break
				}
			}
			if at < 0 {
				continue
			}
			letter := w.Text[at : at+1]
			glued := w.Text[at+1:]
			holder := w
			value := glued
			if glued == "" {
				i++
				holder = record.At(c.Words, i)
				if holder == nil {
					break
				}
				value = holder.Text
			}
			effect := map[string]string{"i": "use", "F": "use", "S": "use", "E": "write"}[letter]
			if client == "sftp" && letter == "b" && value != "-" {
				effect = "read"
			}
			paths := []string{value}
			if letter == "o" {
				setting := match(`(?s)^(\w+)(?:`+text.Space+`*=`+text.Space+`*|`+text.Space+`+)(.+)$`, value)
				paths = nil
				effect = ""
				if setting != nil {
					effect = sshFileOptions[strings.ToLower(setting[1])]
					for _, p := range regexp.MustCompile(`"[^"]*"|`+text.NonSpace+`+`).FindAllString(setting[2], -1) {
						p = strings.ReplaceAll(p, `"`, "")
						for _, prefix := range []string{"%d", "${HOME}"} {
							if p == prefix || strings.HasPrefix(p, prefix+"/") {
								p = "~" + p[len(prefix):]
							}
						}
						paths = append(paths, p)
					}
				}
			}
			if effect == "" {
				continue
			}
			c.Claimed[holder] = true
			for _, p := range paths {
				o := Options{Via: "option"}
				if letter == "o" {
					o.Quoted = new(false)
				}
				ts = append(ts, c.Make(p, holder, effect, o))
			}
		}
		return ts
	}
}
