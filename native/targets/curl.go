package targets

import (
	"regexp"
	"slices"
	"strconv"
	"strings"

	"agentguard/native/record"
)

const CurlValueLetters = "AbcCdDeEFHKmoPQrTtuUwxXyYz"

var curlWrites = strings.Fields("o output D dump-header c cookie-jar etag-save libcurl stderr hsts alt-svc trace trace-ascii ssl-sessions")

func curlTargets(c *Context) []record.Target {
	ts := []record.Target{}
	read := func(p string, w *record.Word, quoted, sends bool) {
		c.Claimed[w] = true
		o := Options{Via: "option", Sends: new(sends)}
		if quoted {
			o.Quoted = new(true)
		}
		ts = append(ts, c.Make(p, w, "read", o))
	}
	remoteName := false
	var outputDir *record.Word
	for i := 0; i < len(c.Words); i++ {
		w := c.Words[i]
		text := w.Text
		if text == "--" {
			break
		}
		remoteName = remoteName || rx(`^(--remote-name(-all)?|-[^-]*O)$`, text)
		if rx(`^--output-dir(=|$)`, text) {
			outputDir = w
			if !strings.Contains(text, "=") {
				i++
				outputDir = record.At(c.Words, i)
			}
			if outputDir != nil {
				c.Claimed[outputDir] = true
			}
			continue
		}
		if url := match(`(?is)^(--url=)?(file:.*)$`, text); url != nil {
			c.Claimed[w] = true
			p := regexp.MustCompile(`(?i)^file:(//)?`).ReplaceAllString(url[2], "")
			p = regexp.MustCompile(`(?i)%([0-9a-f]{2})`).ReplaceAllStringFunc(p, func(s string) string { n, _ := strconv.ParseInt(s[1:], 16, 32); return string(rune(n)) })
			ts = append(ts, c.Make(p, w, "read", Options{Via: "operand", Glob: new(strings.ContainsAny(p, "[{"))}))
			continue
		}
		key, value := "", ""
		long := match(`(?s)^--(data|data-ascii|data-binary|data-urlencode|json|form|header|proxy-header|url-query|variable|upload-file|config|output|dump-header|write-out|cookie|etag-compare|cookie-jar|etag-save|libcurl|stderr|hsts|alt-svc|trace|trace-ascii|ssl-sessions)(=|$)`, text)
		if long != nil {
			key = long[1]
			if at := strings.Index(text, "="); at >= 0 {
				value = text[at+1:]
			} else {
				i++
				value = record.Text(c.Words, i)
			}
		} else if rx(`^-[^-]`, text) {
			for k := 1; k < len(text); k++ {
				if !strings.ContainsRune(CurlValueLetters, rune(text[k])) {
					continue
				}
				key = text[k : k+1]
				value = text[k+1:]
				if value == "" {
					i++
					value = record.Text(c.Words, i)
				}
				break
			}
		}
		from := record.At(c.Words, i)
		if from == nil {
			continue
		}
		switch {
		case rx(`^(d|data|data-ascii|data-binary|data-urlencode|json|H|header|proxy-header|url-query|variable)$`, key):
			if at := strings.Index(value, "@"); at >= 0 && at < len(value)-1 {
				read(value[at+1:], from, true, true)
			}
		case key == "F" || key == "form":
			file := value[strings.Index(value, "=")+1:]
			if strings.HasPrefix(file, "@") || strings.HasPrefix(file, "<") {
				file = file[1:]
			}
			if m := match(`^"([^"]*)"`, file); m != nil {
				file = m[1]
			} else {
				file = strings.Split(file, ";")[0]
			}
			read(file, from, true, true)
		case slices.Contains([]string{"T", "upload-file", "K", "config", "etag-compare"}, key):
			read(value, from, false, true)
		case (key == "w" || key == "write-out") && strings.HasPrefix(value, "@") && len(value) > 1 && value != "@-":
			read(value[1:], from, true, false)
		case (key == "b" || key == "cookie") && value != "" && !strings.Contains(value, "="):
			c.Claimed[from] = true
			ts = append(ts, c.Make(value, from, "use", Options{Via: "option"}))
		case slices.Contains(curlWrites, key) && value != "":
			c.Claimed[from] = true
			if value != "-" {
				ts = append(ts, c.Make(value, from, "write", Options{Via: "option"}))
			}
		}
	}
	if remoteName {
		p := "."
		if outputDir != nil {
			p = strings.TrimPrefix(outputDir.Text, "--output-dir=")
		}
		ts = append(ts, c.Make(p, outputDir, "write", Options{Via: "option", Walk: "none"}))
	}
	return ts
}
