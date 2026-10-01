package harness

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"syscall"
	"testing"
	"time"
)

func TestVerdictAttribution(t *testing.T) {
	fixture := RuntimeCorpus[0]
	input := json.RawMessage(`{"tool_input":{"command":"cat ./public.txt"}}`)
	for _, tc := range []struct {
		name     string
		status   int
		result   *ToolResult
		verdict  string
		conflict bool
	}{
		{"block-before-canary", 0, &ToolResult{Text: "Command blocked by PreToolUse hook: cat PUBLIC_CANARY"}, "deny", true},
		{"execution-after-denial", 2, &ToolResult{Text: "PUBLIC_CANARY"}, "allow", true},
		{"denial-observed", 2, &ToolResult{Text: "DENIED: synthetic"}, "deny", false},
		{"guard-only-is-insufficient", 2, nil, "unverified", false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			v := RuntimeVerdict("codex", []HookObservation{{ProcessResult: ProcessResult{Status: tc.status, Stderr: "DENIED: synthetic"}, Input: input}}, tc.result, fixture)
			if v.Runtime != tc.verdict || v.Conflict != tc.conflict {
				t.Fatalf("%+v", v)
			}
		})
	}
	v := RuntimeVerdict("claude", nil, &ToolResult{Text: "PUBLIC_CANARY"}, fixture)
	if v.Runtime != "unverified" {
		t.Fatalf("missing hook accepted: %+v", v)
	}
}

func TestObservationStreamsAndFailure(t *testing.T) {
	home := t.TempDir()
	entry := filepath.Join(home, "entry")
	for _, status := range []int{0, 2, 7} {
		body := fmt.Sprintf("#!/bin/sh\ncat >/dev/null\nprintf 'synthetic advice\\n'\nprintf 'synthetic reason\\n' >&2\nexit %d\n", status)
		if err := os.WriteFile(entry, []byte(body), 0700); err != nil {
			t.Fatal(err)
		}
		got, err := ObserveHook("claude", entry, home, []byte(`{}`), []string{"HOME=" + home, "PATH=/usr/bin:/bin"}, false)
		if err != nil || got.Status != status || got.Stdout != "synthetic advice\n" || got.Stderr != "synthetic reason\n" || len(got.Survivors) != 0 || got.ObservationError != "" {
			t.Fatalf("%+v %v", got, err)
		}
	}
	got, err := ObserveHook("codex", filepath.Join(home, "missing"), home, []byte(`{}`), []string{"HOME=" + home}, false)
	if err != nil || got.SpawnError == "" || got.Status == 0 {
		t.Fatalf("spawn failure hidden: %+v %v", got, err)
	}
}

func TestProcessTimeoutReapsGroup(t *testing.T) {
	home := t.TempDir()
	pidFile := filepath.Join(home, "child")
	result, err := runProcess([]string{"/bin/sh", "-c", "sleep 20 & child=$!; printf '%s' \"$child\" > child; wait"}, nil, home, []string{"PATH=/usr/bin:/bin"}, 150*time.Millisecond, nil)
	if err != nil || !result.TimedOut || result.Status == 0 || result.Milliseconds > 2000 {
		t.Fatalf("%+v %v", result, err)
	}
	data, err := os.ReadFile(pidFile)
	if err != nil {
		t.Fatal(err)
	}
	var pid int
	if _, err = fmt.Sscanf(string(data), "%d", &pid); err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(time.Second)
	for {
		yes, err := alive(pid)
		if err != nil {
			t.Fatal(err)
		}
		if !yes {
			break
		}
		if time.Now().After(deadline) {
			_ = syscall.Kill(pid, syscall.SIGKILL)
			t.Fatalf("child %d survived timeout", pid)
		}
		time.Sleep(10 * time.Millisecond)
	}
}

func TestOutputBoundary(t *testing.T) {
	out := t.TempDir()
	checkout := filepath.Join(out, "checkout")
	if err := os.MkdirAll(filepath.Join(checkout, ".git"), 0700); err != nil {
		t.Fatal(err)
	}
	alias := filepath.Join(out, "alias")
	if err := os.Symlink(checkout, alias); err != nil {
		t.Fatal(err)
	}
	for _, path := range []string{filepath.Join(checkout, "report"), filepath.Join(alias, "report")} {
		if _, err := newOutput(path); err == nil {
			t.Fatalf("accepted checkout output %s", path)
		}
	}
	path := filepath.Join(out, "evidence")
	if _, err := newOutput(path); err != nil {
		t.Fatal(err)
	}
	if _, err := newOutput(path); err == nil {
		t.Fatal("overwrote evidence directory")
	}
}

func TestRuntimeDriverCompleteAndMissingHook(t *testing.T) {
	root, err := filepath.Abs("../..")
	if err != nil {
		t.Fatal(err)
	}
	out := t.TempDir()
	helper := filepath.Join(out, "runtime")
	goBinary := os.Getenv("GO")
	if goBinary == "" {
		goBinary = "go"
	}
	build := exec.Command(goBinary, "build", "-o", helper, "./cmd/agent-guard-runtime")
	build.Dir = root
	if output, err := build.CombinedOutput(); err != nil {
		t.Fatalf("build helper: %v %s", err, output)
	}
	bin := filepath.Join(out, "clients")
	if err = os.Mkdir(bin, 0700); err != nil {
		t.Fatal(err)
	}
	self, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	for _, runtime := range []string{"claude", "pi", "codex"} {
		launcher := "#!/bin/sh\nif test \"$1\" = --version; then printf 'synthetic runtime 1\\n'; exit 0; fi\nexport GORACE=atexit_sleep_ms=0\nexec " + quote(self) + " -test.run=TestSyntheticRuntimeClient -- " + runtime + " \"$@\"\n"
		if err = os.WriteFile(filepath.Join(bin, runtime), []byte(launcher), 0700); err != nil {
			t.Fatal(err)
		}
	}
	t.Setenv("PATH", bin+":/usr/bin:/bin")
	packageBin := filepath.Join(out, "package/bin")
	if err = os.MkdirAll(packageBin, 0700); err != nil {
		t.Fatal(err)
	}
	entry := filepath.Join(packageBin, "agent-guard")
	guard := "#!/bin/sh\nbody=$(cat)\ncase \"$body\" in *Library*|*.ssh*|*find*) printf 'DENIED: synthetic rule\\n' >&2; exit 2;; esac\n"
	if err = os.WriteFile(entry, []byte(guard), 0700); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(packageBin, "agent-guard-native"), []byte("synthetic binding only"), 0700); err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"missing", "other"} {
		selected := filepath.Join(packageBin, name)
		if name == "other" {
			if err := os.WriteFile(selected, []byte(guard), 0700); err != nil {
				t.Fatal(err)
			}
		}
		directory := filepath.Join(out, "invalid-entry-"+name)
		if err := RunRuntime(RuntimeOptions{Source: root, Entry: selected, Helper: helper, Output: directory, Runtimes: []string{"codex"}}); err == nil {
			t.Fatalf("selected entry %s was replaced by its sibling", selected)
		}
		if _, err := os.Stat(directory); !os.IsNotExist(err) {
			t.Fatal("invalid entry created evidence before validation")
		}
	}
	alias := filepath.Join(out, "entry-alias")
	if err := os.Symlink(entry, alias); err != nil {
		t.Fatal(err)
	}
	for _, ablate := range []bool{false, true} {
		directory := filepath.Join(out, fmt.Sprintf("report-%t", ablate))
		selected := entry
		if !ablate {
			selected = alias
		}
		err = RunRuntime(RuntimeOptions{Source: root, Entry: selected, Helper: helper, Output: directory, Runtimes: []string{"claude", "pi", "codex"}, Ablate: ablate})
		if err != nil {
			t.Fatal(err)
		}
		var report struct {
			Records  []RuntimeRow     `json:"records"`
			Summary  []RuntimeSummary `json:"summary"`
			Manifest []Binding        `json:"manifest"`
			Clients  []RuntimeClient  `json:"clients"`
		}
		body, err := os.ReadFile(filepath.Join(directory, "report.json"))
		if err != nil {
			t.Fatal(err)
		}
		if err = json.Unmarshal(body, &report); err != nil {
			t.Fatal(err)
		}
		if len(report.Records) != 90 || len(report.Manifest) != 2 || len(report.Clients) != 3 {
			t.Fatalf("incomplete report: %d rows %d bindings", len(report.Records), len(report.Manifest))
		}
		for _, client := range report.Clients {
			physicalBin, err := filepath.EvalSymlinks(bin)
			if err != nil {
				t.Fatal(err)
			}
			if len(client.Executable.SHA256) != 64 || !strings.HasPrefix(client.Executable.Path, physicalBin+"/") || client.Version.Status != 0 || client.Version.Stdout != "synthetic runtime 1\n" {
				t.Fatalf("client identity incomplete: %+v", client)
			}
		}
		for _, summary := range report.Summary {
			if !summary.Complete || summary.Verified != 30 || summary.Matched != 30 {
				t.Fatalf("%+v", summary)
			}
		}
	}
	if err = os.WriteFile(filepath.Join(bin, "codex"), []byte("#!/bin/sh\nprintf 'SYNTHETIC_RUNTIME_CLIENT\\n'\nexit 7\n"), 0700); err != nil {
		t.Fatal(err)
	}
	directory := filepath.Join(out, "missing-hook")
	if err = RunRuntime(RuntimeOptions{Source: root, Entry: entry, Helper: helper, Output: directory, Runtimes: []string{"codex"}}); err == nil {
		t.Fatal("missing hooks falsely passed")
	}
	var report struct {
		Summary []RuntimeSummary `json:"summary"`
		Records []RuntimeRow     `json:"records"`
	}
	body, err := os.ReadFile(filepath.Join(directory, "report.json"))
	if err != nil {
		t.Fatal(err)
	}
	if err = json.Unmarshal(body, &report); err != nil {
		t.Fatal(err)
	}
	if len(report.Records) != 1 || report.Summary[0].Planned != 30 || report.Summary[0].Verified != 0 || report.Summary[0].FalseAllow != nil || report.Summary[0].FalseDeny != nil {
		t.Fatalf("missing evidence converted into verdicts: %+v", report.Summary)
	}
}

// This subprocess implements the external CLI boundary; the production driver,
// HTTP server, hook executable, and evidence writer all run unchanged.
func TestSyntheticRuntimeClient(t *testing.T) {
	args := os.Args
	index := -1
	for i, arg := range args {
		if arg == "--" {
			index = i
			break
		}
	}
	if index < 0 {
		return
	}
	runtime := args[index+1]
	if os.Getenv("HOME") == "" || os.Getenv("ANTHROPIC_API_KEY") != "synthetic-not-a-credential" {
		os.Exit(81)
	}
	url := os.Getenv("AGENT_GUARD_TEST_MODEL_URL") + "/v1/messages"
	request := map[string]any{"model": "synthetic", "stream": true, "messages": []any{map[string]any{"role": "user", "content": "Run"}}}
	if runtime == "codex" {
		url = os.Getenv("AGENT_GUARD_TEST_MODEL_URL") + "/v1/responses"
		request = map[string]any{"model": "synthetic", "input": []any{}}
	}
	post := func(body any) []byte {
		encoded, _ := json.Marshal(body)
		response, err := http.Post(url, "application/json", bytes.NewReader(encoded))
		if err != nil {
			panic(err)
		}
		defer response.Body.Close()
		data, err := io.ReadAll(response.Body)
		if err != nil {
			panic(err)
		}
		return data
	}
	first := post(request)
	command := ""
	for _, line := range strings.Split(string(first), "\n") {
		if !strings.HasPrefix(line, "data: ") {
			continue
		}
		var event map[string]json.RawMessage
		if json.Unmarshal([]byte(strings.TrimPrefix(line, "data: ")), &event) != nil {
			os.Exit(82)
		}
		if runtime == "codex" {
			var item struct {
				Arguments string `json:"arguments"`
			}
			if json.Unmarshal(event["item"], &item) == nil && item.Arguments != "" {
				var value struct {
					Cmd string `json:"cmd"`
				}
				if json.Unmarshal([]byte(item.Arguments), &value) != nil {
					os.Exit(83)
				}
				command = value.Cmd
			}
		} else {
			var delta struct {
				Partial string `json:"partial_json"`
			}
			if json.Unmarshal(event["delta"], &delta) == nil && delta.Partial != "" {
				var value struct {
					Command string `json:"command"`
				}
				if json.Unmarshal([]byte(delta.Partial), &value) != nil {
					os.Exit(84)
				}
				command = value.Command
			}
		}
	}
	if command == "" {
		os.Exit(85)
	}
	event, _ := json.Marshal(map[string]any{"tool_name": "Bash", "tool_input": map[string]string{"command": command}})
	hook := exec.Command(os.Getenv("AGENT_GUARD_TEST_HELPER"), "--hook", runtime)
	hook.Stdin = bytes.NewReader(event)
	var stderr bytes.Buffer
	hook.Stderr = &stderr
	err := hook.Run()
	text := ""
	isError := err != nil
	if isError {
		if runtime == "codex" {
			text = "Command blocked by PreToolUse hook: "
		}
		text += stderr.String()
	} else {
		result, err := exec.Command("/bin/sh", "-c", command).CombinedOutput()
		if err != nil {
			os.Exit(86)
		}
		text = string(result)
	}
	if runtime == "codex" {
		request["input"] = []any{map[string]any{"type": "function_call_output", "output": text}}
	} else {
		request["messages"] = []any{map[string]any{"role": "user", "content": []any{map[string]any{"type": "tool_result", "tool_use_id": "tool_harness", "is_error": isError, "content": text}}}}
	}
	second := post(request)
	if !bytes.Contains(second, []byte("done")) {
		os.Exit(87)
	}
	os.Exit(0)
}
