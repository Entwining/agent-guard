package harness

import (
	"encoding/json"
	"errors"
	"fmt"
	"net"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
	"time"
)

type RuntimeOptions struct {
	Source, Entry, Output, Helper string
	Runtimes                      []string
	Ablate                        bool
}

type RuntimeClient struct {
	Runtime       string        `json:"runtime"`
	Executable    Binding       `json:"executable"`
	Version       ProcessResult `json:"version"`
	IdentityError string        `json:"identityError,omitempty"`
}

type RuntimeRow struct {
	Runtime  string `json:"runtime"`
	Run      int    `json:"run"`
	ID       string `json:"id"`
	Command  string `json:"command"`
	Expected string `json:"expected"`
	Verdict
	ProcessResult
	Hooks      []HookObservation `json:"hooks"`
	Result     *ToolResult       `json:"runtimeResult"`
	ModelError string            `json:"modelError,omitempty"`
	TraceError string            `json:"traceError,omitempty"`
	Requests   int               `json:"requests"`
}

type RuntimeSummary struct {
	Runtime       string   `json:"runtime"`
	Planned       int      `json:"plannedCalls"`
	Attempted     int      `json:"attemptedCalls"`
	Verified      int      `json:"verified"`
	Matched       int      `json:"matchedCalls"`
	FalseAllow    *int     `json:"falseAllow"`
	FalseDeny     *int     `json:"falseDeny"`
	Disagreements int      `json:"runtimeGuardDisagreements"`
	HookP50       *float64 `json:"guardP50Ms"`
	HookP95       *float64 `json:"guardP95Ms"`
	Complete      bool     `json:"complete"`
}

func runtimeSummaries(rows []RuntimeRow, runtimes []string, ablate bool) []RuntimeSummary {
	var summaries []RuntimeSummary
	for _, runtime := range runtimes {
		s := RuntimeSummary{Runtime: runtime, Planned: len(RuntimeCorpus) * 3}
		var times []float64
		valid := true
		for _, row := range rows {
			if row.Runtime != runtime {
				continue
			}
			s.Attempted++
			if row.Verdict.Runtime != "unverified" {
				s.Verified++
				if row.Expected == "allow" {
					if s.FalseDeny == nil {
						value := 0
						s.FalseDeny = &value
					}
					if row.Verdict.Runtime == "deny" {
						(*s.FalseDeny)++
					}
				}
				if row.Expected == "deny" {
					if s.FalseAllow == nil {
						value := 0
						s.FalseAllow = &value
					}
					if row.Verdict.Runtime == "allow" {
						(*s.FalseAllow)++
					}
				}
			}
			expected := row.Expected
			if ablate {
				expected = "allow"
			}
			if row.Verdict.Runtime == expected {
				s.Matched++
			}
			if row.Conflict {
				s.Disagreements++
			}
			valid = valid && row.Status == 0 && !row.TimedOut && row.SpawnError == "" && row.WaitError == "" && row.ModelError == "" && row.TraceError == "" && len(row.Hooks) == 1
			for _, hook := range row.Hooks {
				times = append(times, hook.Milliseconds)
				valid = valid && !hook.TimedOut && hook.SpawnError == "" && hook.WaitError == "" && hook.ObservationError == "" && len(hook.Survivors) == 0 && (hook.Status == 0 || hook.Status == 2)
			}
		}
		sort.Float64s(times)
		if len(times) > 0 {
			p50, p95 := times[(len(times)-1)/2], times[(len(times)*95+99)/100-1]
			s.HookP50, s.HookP95 = &p50, &p95
		}
		s.Complete = valid && s.Attempted == s.Planned && s.Verified == s.Planned && s.Matched == s.Planned && s.Disagreements == 0
		summaries = append(summaries, s)
	}
	return summaries
}

func RunRuntime(options RuntimeOptions) error {
	if !filepath.IsAbs(options.Entry) || !filepath.IsAbs(options.Helper) {
		return errors.New("entry and helper must be absolute paths")
	}
	entry, err := filepath.EvalSymlinks(options.Entry)
	if err != nil {
		return err
	}
	if filepath.Base(entry) != "agent-guard" || filepath.Base(filepath.Dir(entry)) != "bin" {
		return errors.New("select the assembled bin/agent-guard executable")
	}
	if len(options.Runtimes) == 0 {
		return errors.New("select claude, pi, or codex")
	}
	for _, runtime := range options.Runtimes {
		if runtime != "claude" && runtime != "pi" && runtime != "codex" {
			return fmt.Errorf("unknown runtime %q", runtime)
		}
	}
	output, err := newOutput(options.Output)
	if err != nil {
		return err
	}
	source, err := SourceBindings(options.Source)
	if err != nil {
		return err
	}
	home, err := syntheticHome(output)
	if err != nil {
		return err
	}
	defer os.RemoveAll(home)
	var manifest []Binding
	for _, name := range []string{"agent-guard", "agent-guard-native"} {
		from := entry
		if name == "agent-guard-native" {
			from = filepath.Join(filepath.Dir(entry), name)
		}
		to := filepath.Join(home, "installation/bin", name)
		if err = copyFile(from, to); err != nil {
			return err
		}
		binding, err := hashFile(to, "bin/"+name)
		if err != nil {
			return err
		}
		manifest = append(manifest, binding)
	}
	helperBinding, err := hashFile(options.Helper, "runtime-harness")
	if err != nil {
		return err
	}
	model := &ScriptedModel{}
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		return err
	}
	server := &http.Server{Handler: model, ReadHeaderTimeout: 5 * time.Second, ReadTimeout: 20 * time.Second, WriteTimeout: 20 * time.Second}
	serverResult := make(chan error, 1)
	go func() { serverResult <- server.Serve(listener) }()
	defer func() { _ = server.Close(); <-serverResult }()
	url := "http://" + listener.Addr().String()
	if err = runtimeConfig(home, options.Helper, url); err != nil {
		return err
	}
	trace := filepath.Join(home, "trace.jsonl")
	ablate := "0"
	if options.Ablate {
		ablate = "1"
	}
	env := []string{"HOME=" + home, "PATH=" + os.Getenv("PATH"), "TMPDIR=" + home, "CODEX_HOME=" + filepath.Join(home, ".codex"), "PI_CODING_AGENT_DIR=" + filepath.Join(home, ".pi"), "CLAUDE_CONFIG_DIR=" + filepath.Join(home, ".claude"), "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1", "ANTHROPIC_BASE_URL=" + url, "ANTHROPIC_API_KEY=synthetic-not-a-credential", "AGENT_GUARD_TEST_MODEL_URL=" + url, "AGENT_GUARD_TEST_ENTRY=" + filepath.Join(home, "installation/bin/agent-guard"), "AGENT_GUARD_TEST_HOOK_TRACE=" + trace, "AGENT_GUARD_TEST_ABLATE=" + ablate, "AGENT_GUARD_TEST_HELPER=" + options.Helper}
	records := []RuntimeRow{}
	clients := []RuntimeClient{}
	file, err := os.OpenFile(filepath.Join(output, "records.jsonl"), os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		return err
	}
	defer file.Close()
	for _, runtime := range options.Runtimes {
		client, lookupErr := exec.LookPath(runtime)
		identity := RuntimeClient{Runtime: runtime}
		if lookupErr != nil {
			client = runtime
			identity.IdentityError = lookupErr.Error()
		} else {
			resolved, resolveErr := filepath.EvalSymlinks(client)
			if resolveErr != nil {
				return resolveErr
			}
			identity.Executable, err = hashFile(resolved, resolved)
			if err != nil {
				return err
			}
		}
		identity.Version, err = runProcess([]string{client, "--version"}, nil, filepath.Join(home, "workspace"), env, 5*time.Second, nil)
		if err != nil {
			return err
		}
		clients = append(clients, identity)
	runtimeCalls:
		for repeat := 0; repeat < 3; repeat++ {
			for _, fixture := range RuntimeCorpus {
				model.Begin(runtime, fixture.Command)
				if err = os.WriteFile(trace, nil, 0600); err != nil {
					return err
				}
				args := runtimeArgs(runtime, client, home)
				process, err := runProcess(args, nil, filepath.Join(home, "workspace"), env, 20*time.Second, nil)
				if err != nil {
					return err
				}
				hooks, traceErr := readHooks(trace)
				result, requests, modelError := model.Result()
				row := RuntimeRow{Runtime: runtime, Run: repeat, ID: fixture.ID, Command: fixture.Command, Expected: fixture.Expected, Verdict: RuntimeVerdict(runtime, hooks, result, fixture), ProcessResult: process, Hooks: hooks, Result: result, Requests: requests, ModelError: modelError}
				if traceErr != nil {
					row.TraceError = traceErr.Error()
				}
				records = append(records, row)
				if err = json.NewEncoder(file).Encode(row); err != nil {
					return err
				}
				if err = file.Sync(); err != nil {
					return err
				}
				fmt.Printf("%s %d %s %s status=%d hooks=%d\n", runtime, repeat, fixture.ID, row.Verdict.Runtime, row.Status, len(hooks))
				if (row.Status != 0 || row.TimedOut || row.SpawnError != "") && requests == 0 && len(hooks) == 0 {
					break runtimeCalls
				}
			}
		}
	}
	summary := runtimeSummaries(records, options.Runtimes, options.Ablate)
	for i, client := range clients {
		if client.IdentityError != "" || client.Version.Status != 0 || client.Version.TimedOut || client.Version.WaitError != "" {
			summary[i].Complete = false
		}
	}
	report := struct {
		Manifest []Binding        `json:"manifest"`
		Source   []Binding        `json:"source"`
		Helper   Binding          `json:"helper"`
		Clients  []RuntimeClient  `json:"clients"`
		Ablated  bool             `json:"ablated"`
		Summary  []RuntimeSummary `json:"summary"`
		Records  []RuntimeRow     `json:"records"`
		Survival string           `json:"survivalCoverage"`
	}{manifest, source, helperBinding, clients, options.Ablate, summary, records, "Entry PID only. Runner/checker/watchdog/descendant cleanup requires the separately instrumented lifecycle driver."}
	if err = writeJSON(filepath.Join(output, "report.json"), report); err != nil {
		return err
	}
	for _, s := range summary {
		if !s.Complete {
			return errors.New("runtime evaluation is incomplete or mismatched; see report.json")
		}
	}
	return nil
}

func runtimeArgs(runtime, client, home string) []string {
	switch runtime {
	case "pi":
		return []string{client, "--no-extensions", "--no-session", "-e", filepath.Join(home, "runtime-pi.mjs"), "--provider", "harness", "--model", "synthetic", "--mode", "json", "-p", "Run the requested tool."}
	case "claude":
		return []string{client, "-p", "Run the requested tool.", "--model", "claude-sonnet-4-5", "--output-format", "stream-json", "--verbose", "--dangerously-skip-permissions"}
	default:
		return []string{client, "--dangerously-bypass-hook-trust", "exec", "--skip-git-repo-check", "--json", "Run the requested tool."}
	}
}

func runtimeConfig(home, helper, url string) error {
	for _, runtime := range []string{"claude", "codex"} {
		matcher := "Bash"
		if runtime == "codex" {
			matcher = "^Bash$"
		}
		config := map[string]any{"hooks": map[string]any{"PreToolUse": []any{map[string]any{"matcher": matcher, "hooks": []any{map[string]any{"type": "command", "command": quote(helper) + " --hook " + runtime, "timeout": 5}}}}}}
		path := filepath.Join(home, ".claude/settings.json")
		if runtime == "codex" {
			path = filepath.Join(home, ".codex/hooks.json")
		}
		if err := writeJSON(path, config); err != nil {
			return err
		}
	}
	config := fmt.Sprintf("model = \"synthetic\"\nmodel_provider = \"harness\"\napproval_policy = \"on-request\"\napprovals_reviewer = \"auto_review\"\ndefault_permissions = \"development\"\n[features]\nhooks = true\n[permissions.development.filesystem]\n\":minimal\" = \"read\"\n%q = \"write\"\n[permissions.development.network]\nenabled = true\n[model_providers.harness]\nname = \"Synthetic local harness\"\nbase_url = %q\nwire_api = \"responses\"\nrequires_openai_auth = false\n", home, url+"/v1")
	if err := os.WriteFile(filepath.Join(home, ".codex/config.toml"), []byte(config), 0600); err != nil {
		return err
	}
	return os.WriteFile(filepath.Join(home, "runtime-pi.mjs"), []byte(strings.TrimSpace(piAdapter)+"\n"), 0600)
}

const piAdapter = `
import { spawnSync } from "node:child_process";
export default function (pi) {
  pi.registerProvider("harness", { baseUrl: process.env.AGENT_GUARD_TEST_MODEL_URL, apiKey: "synthetic-not-a-credential", api: "anthropic-messages", models: [{ id: "synthetic", name: "Synthetic", reasoning: false, input: ["text"], cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 }, contextWindow: 200000, maxTokens: 4096 }] });
  pi.on("tool_call", (event, ctx) => {
    if (event.toolName !== "bash") return;
    const input = { tool_name: "bash", tool_input: event.input, cwd: ctx.cwd };
    const result = spawnSync(process.env.AGENT_GUARD_TEST_HELPER, ["--hook", "pi"], { cwd: ctx.cwd, input: JSON.stringify(input), encoding: "utf8", timeout: 5000 });
    if (result.status === 0) return;
    return { block: true, reason: result.stderr?.trim() || result.error?.message || "agent-guard failed" };
  });
}
`
