package targets

import (
	jsText "agentguard/native"
	"agentguard/native/record"
	"slices"
	"strings"
)

var wgetShort = map[string]string{"i": "input-file", "O": "output-document", "e": "execute", "P": "directory-prefix", "o": "output-file", "a": "append-output"}
var wgetWrites = strings.Fields("output-document output-file append-output directory-prefix save-cookies warc-file hsts-file")
var wgetrc = map[string]string{"postfile": "post-file", "bodyfile": "body-file", "input": "input-file", "outputdocument": "output-document", "logfile": "output-file", "dirprefix": "directory-prefix", "loadcookies": "load-cookies", "savecookies": "save-cookies", "warcfile": "warc-file", "hstsfile": "hsts-file"}

func wgetTargets(c *Context) []record.Target {
	ts := []record.Target{}
	placed := false
	for i, w := range c.Words {
		text := w.Text
		if text == "--spider" {
			placed = true
		}
		long := match(`^--(post-file|body-file|input-file|output-document|output-file|append-output|directory-prefix|save-cookies|warc-file|hsts-file|execute|config|load-cookies)(=|$)`, text)
		short := match(`^-[A-Za-z]*?([ieOPoa])(.*)$`, text)
		key := ""
		glued := false
		if long != nil {
			key = long[1]
			glued = strings.Contains(text, "=")
		} else if short != nil {
			key = wgetShort[short[1]]
			glued = short[2] != ""
		}
		if key == "" {
			continue
		}
		holder := w
		p := ""
		if !glued {
			holder = record.At(c.Words, i+1)
			if holder == nil {
				continue
			}
			p = holder.Text
		} else if long != nil {
			p = text[strings.Index(text, "=")+1:]
		} else {
			p = short[2]
		}
		if key == "execute" {
			m := match(`^`+jsText.Space+`*([A-Za-z_-]+)`+jsText.Space+`*=`+jsText.Space+`*(.*)$`, p)
			if m == nil {
				continue
			}
			key = wgetrc[strings.NewReplacer("_", "", "-", "").Replace(strings.ToLower(m[1]))]
			if key == "" {
				continue
			}
			p = m[2]
		}
		c.Claimed[holder] = true
		placed = placed || key == "output-document" || key == "directory-prefix"
		if key == "output-document" && p == "-" {
			continue
		}
		e := "read"
		if slices.Contains(wgetWrites, key) {
			e = "write"
		}
		ts = append(ts, c.Make(p, holder, e, Options{Via: "option", Walk: "none", Sends: B(slices.Contains([]string{"post-file", "body-file", "config"}, key))}))
	}
	if !placed {
		ts = append(ts, c.Make(".", nil, "write", Options{Via: "option", Walk: "none"}))
	}
	return ts
}
