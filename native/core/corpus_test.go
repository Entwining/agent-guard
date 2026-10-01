package core

import (
	"agentguard/native/filesystem"
	"bufio"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"regexp"
	"strings"
	"testing"
)

type contractRow struct {
	Family, Tool, Input, Cwd, Glob string
	Index                          int
	Expected                       map[string]struct {
		Reason string
		Advice []string
	}
}

func TestContractCorpus(t *testing.T) {
	file, err := os.Open("../../tests/fixtures/contract.jsonl")
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	home := contractHome(t)
	root := filepath.Dir(home)
	t.Setenv("USER", "fixture-user")
	placeholder := regexp.MustCompile(`\$[HU]\b`)
	expand := func(value string) string {
		return strings.ReplaceAll(strings.ReplaceAll(value, "$H", home), "$R", root)
	}
	expandCommand := func(value string) string {
		return placeholder.ReplaceAllStringFunc(value, func(match string) string {
			replacement := home
			if match[:2] == "$U" {
				replacement = "fixture-user"
			}
			return replacement
		})
	}
	scanner := bufio.NewScanner(file)
	scanner.Buffer(make([]byte, 65536), 1<<20)
	rows := 0
	for scanner.Scan() {
		var row contractRow
		if err := json.Unmarshal(scanner.Bytes(), &row); err != nil {
			t.Fatal(err)
		}
		rows++
		tool := strings.ToLower(row.Tool)
		input := expand(row.Input)
		if tool == "bash" {
			input = expandCommand(row.Input)
		}
		cwd := expand(row.Cwd)
		for _, runtime := range []string{"claude", "codex", "pi"} {
			t.Run(fmt.Sprintf("%s/%d/%s", row.Family, row.Index, runtime), func(t *testing.T) {
				want, exists := row.Expected[runtime]
				if !exists {
					t.Fatal("missing runtime expectation")
				}
				req := BuildRequest(runtime, tool, cwd, input, row.Glob, home)
				probe := &watchingProbe{}
				got := verdict(t, req, probe)
				if got != want.Reason || !reflect.DeepEqual(Suggestions(req), want.Advice) {
					t.Fatalf("reason/advice %q %q; want %q %q", got, Suggestions(req), want.Reason, want.Advice)
				}
				assertCorpusProbeScope(t, home, probe)
				field := "file_path"
				if tool == "bash" {
					field = "command"
				} else if tool == "grep" {
					field = "path"
				}
				body, err := json.Marshal(map[string]any{
					"tool_name": row.Tool, "cwd": cwd,
					"tool_input": map[string]string{field: input, "glob": row.Glob},
				})
				if err != nil {
					t.Fatal(err)
				}
				result, err := CheckEvent(t.Context(), runtime, cwd, home, body)
				if err != nil {
					t.Fatal(err)
				}
				exit, stdout, stderr := 0, "", ""
				if want.Reason != "" {
					exit, stderr = 2, want.Reason
					if runtime == "claude" {
						stderr = "DENIED: " + stderr + " Do NOT bypass this restriction or retry the same blocked command."
					}
					stderr += "\n"
				} else if len(want.Advice) != 0 {
					advice, err := json.Marshal(strings.Join(want.Advice, "\n"))
					if err != nil {
						t.Fatal(err)
					}
					stdout = `{"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":` + string(advice) + "}}\n"
				}
				if result.Exit != exit || string(result.Stdout) != stdout || string(result.Stderr) != stderr {
					t.Fatalf("public result %+v; want exit %d stdout %q stderr %q", result, exit, stdout, stderr)
				}
			})
		}
	}
	if err := scanner.Err(); err != nil {
		t.Fatal(err)
	}
	if rows == 0 {
		t.Fatal("empty contract corpus")
	}
}

func assertCorpusProbeScope(t *testing.T, home string, probe *watchingProbe) {
	t.Helper()
	if len(probe.violations) != 0 {
		t.Fatalf("protected traversal: %v", probe.violations)
	}
	for _, path := range append(probe.readlinks, probe.stats...) {
		if filesystem.IsAppdata(filesystem.Unfirmlink(path), home) {
			t.Fatalf("probe inside App Data: %s", path)
		}
	}
	inScope := func(path string) bool {
		resolved := strings.ToLower(strings.TrimSuffix(resolveExisting(t, path), "/"))
		ssh := strings.ToLower(home + "/.ssh")
		return strings.HasPrefix(resolved+"/", ssh+"/") || strings.HasPrefix(ssh+"/", resolved+"/")
	}
	for _, path := range probe.stats {
		if inScope(path) {
			continue
		}
		holdsScopedPath := false
		for _, other := range probe.stats {
			holdsScopedPath = holdsScopedPath || strings.HasPrefix(other, path+"/") && inScope(other)
		}
		if !holdsScopedPath {
			t.Fatalf("stat outside SSH identity scope: %s", path)
		}
	}
}

func contractHome(t *testing.T) string {
	t.Helper()
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	home := root + "/home"
	var fixture struct {
		Directories []string
		Files       []string
		Links       [][2]string
	}
	data, err := os.ReadFile("../../tests/fixtures/filesystem.json")
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(data, &fixture); err != nil {
		t.Fatal(err)
	}
	for _, path := range fixture.Directories {
		if err := os.MkdirAll(home+"/"+path, 0755); err != nil {
			t.Fatal(err)
		}
	}
	for _, tree := range filesystem.AppdataTrees {
		if err := os.MkdirAll(home+"/Library/"+tree+"/com.x", 0755); err != nil {
			t.Fatal(err)
		}
	}
	for _, path := range fixture.Files {
		if err := os.WriteFile(home+"/"+path, nil, 0600); err != nil {
			t.Fatal(err)
		}
	}
	for _, link := range fixture.Links {
		target := strings.ReplaceAll(link[1], "$H", home)
		if err := os.Symlink(target, home+"/"+link[0]); err != nil {
			t.Fatal(err)
		}
	}
	climb := strings.Repeat("../", len(strings.Split(home+"/project", "/"))) + strings.TrimPrefix(home, "/") + "/project/data-link"
	if err := os.Symlink(climb, home+"/project/firmlink-climb"); err != nil {
		t.Fatal(err)
	}
	dirs := []string{"project/locked"}
	for _, tree := range filesystem.AppdataTrees {
		dirs = append(dirs, "Library/"+tree)
	}
	for _, dir := range dirs {
		path := home + "/" + dir
		if err := os.Chmod(path, 0); err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = os.Chmod(path, 0755) })
	}
	return home
}
