package core

import (
	"encoding/json"
	"strings"
	"testing"

	"agentguard/native/reasons"
)

func TestProtocol(t *testing.T) {
	home := syntheticHome(t)
	for _, runtime := range []string{"claude", "codex", "pi"} {
		t.Run(runtime, func(t *testing.T) {
			for _, c := range []struct {
				name, body string
				exit       int
				out, err   string
			}{
				{"safe", `{"tool_input":{"command":"ls"}}`, 0, "", ""},
				{"unknown", `{"tool_name":"WebFetch","tool_input":{}}`, 0, "", ""},
				{"grep-default", `{"tool_name":"Grep","tool_input":{"pattern":"x"}}`, 0, "", ""},
				{"denial", `{"tool_input":{"command":"env"}}`, 2, "", reasons.Dump},
				{"syntax", `{"tool_input":{"command":"for x (a b); do ls; done"}}`, 2, "", reasons.Syntax},
				{"advice", `{"tool_input":{"command":"rg -rn foo src"}}`, 0, reasons.Replace, ""},
			} {
				t.Run(c.name, func(t *testing.T) {
					r, e := CheckEvent(t.Context(), runtime, home+"/project", home, []byte(c.body))
					if e != nil {
						t.Fatal(e)
					}
					if r.Exit != c.exit {
						t.Fatal(r.Exit)
					}
					err := c.err
					if err != "" {
						if runtime == "claude" {
							err = "DENIED: " + err + " Do NOT bypass this restriction or retry the same blocked command."
						}
						err += "\n"
					}
					if string(r.Stderr) != err {
						t.Fatalf("stderr %q, want %q", r.Stderr, err)
					}
					out := ""
					if c.out != "" && runtime == "claude" {
						context, _ := json.Marshal(c.out)
						out = `{"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":` + string(context) + "}}\n"
					}
					if string(r.Stdout) != out {
						t.Fatalf("stdout %q, want %q", r.Stdout, out)
					}
				})
			}
		})
	}
	t.Run("cwd-precedence", func(t *testing.T) {
		body := `{"cwd":` + quote(home) + `,"tool_input":{"cwd":` + quote(home+"/project") + `,"command":"ls Library/Containers"}}`
		r, e := CheckEvent(t.Context(), "codex", home+"/project", home, []byte(body))
		if e != nil || r.Exit != 2 || !strings.Contains(string(r.Stderr), reasons.Appdata) {
			t.Fatalf("%+v %v", r, e)
		}
		body = `{"tool_input":{"cwd":` + quote(home) + `,"command":"ls Library/Containers"}}`
		r, e = CheckEvent(t.Context(), "codex", home+"/project", home, []byte(body))
		if e != nil || r.Exit != 2 {
			t.Fatalf("%+v %v", r, e)
		}
	})
	t.Run("malformed", func(t *testing.T) {
		for _, body := range []string{`null`, `[]`, `{`, `{"tool_name":"Bash"}`, `{"tool_input":"cat .env"}`, `{"tool_input":{"command":["cat",".env"]}}`, `{"tool_input":{}}`, `{"tool_name":"Read","tool_input":{"path":".env"}}`, `{"tool_name":"Grep","tool_input":{"path":5}}`} {
			if _, e := CheckEvent(t.Context(), "claude", home+"/project", home, []byte(body)); e == nil {
				t.Fatalf("malformed event accepted: %s", body)
			}
		}
	})
	t.Run("unknown-and-grep-primitives", func(t *testing.T) {
		for _, body := range []string{
			`{"tool_name":"WebFetch","tool_input":"unused"}`,
			`{"tool_name":"WebFetch","tool_input":[]}`,
			`{"tool_name":"WebFetch","cwd":` + quote(home+"/project") + `}`,
			`{"tool_name":"Grep","tool_input":5}`,
		} {
			r, e := CheckEvent(t.Context(), "claude", home+"/project", home, []byte(body))
			if e != nil || r.Exit != 0 || len(r.Stdout) != 0 || len(r.Stderr) != 0 {
				t.Fatalf("frozen unknown/default-path contract changed: %s: %+v %v", body, r, e)
			}
		}
	})
}

func quote(s string) string { b, _ := json.Marshal(s); return string(b) }
