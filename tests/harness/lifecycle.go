package harness

import (
	_ "embed"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"
	"time"
)

//go:embed testdata/fault.go.txt
var faultSource string

var LifecycleFaults = []string{"pipe", "late-start", "startup-stall", "large-reason", "large-stderr", "slow-reason", "no-reason", "checker-failure", "panic", "partial-panic", "hang", "runner-stall", "early-failure", "leftover", "dependency-failure"}

var LifecycleControls = map[string]string{"drain": "large-reason", "stderr-drain": "large-stderr", "failclosed": "checker-failure", "deadline": "runner-stall", "cleanup": "leftover", "dependency": "dependency-failure"}

type ProcessRecord struct {
	Role string `json:"role"`
	PID  int    `json:"pid"`
	PGID int    `json:"pgid"`
}

type LifecycleResult struct {
	ProcessResult
	Processes      []ProcessRecord `json:"processes"`
	AliveAtReturn  []ProcessRecord `json:"alive_at_return"`
	AliveAfterPoll []ProcessRecord `json:"alive_after_poll"`
	ProducerBytes  int             `json:"producer_bytes"`
}

type faultPatch struct{ Runner, Entry, Links, Fixture string }

func injectFault(original, entry, links, fault, control string) (faultPatch, error) {
	var failure error
	replace := func(text, old, next string) string {
		if strings.Count(text, old) != 1 {
			failure = errors.Join(failure, fmt.Errorf("fault injection marker count differs: %q", old))
			return text
		}
		return strings.Replace(text, old, next, 1)
	}
	runner := replace(original, "func check(args []string) int {", "func check(args []string) int {\nreturn faultChecker(args)\n/*")
	runner = replace(runner, "\treturn result.Exit\n}", "\treturn result.Exit\n*/\n}")
	if fault == "dependency-failure" {
		runner = replace(original, "func check(args []string) int {", "func check(args []string) int {\nlogProcess(\"checker\", os.Getpid())\n")
		links = replace(links, "firmlinkError = e", "firmlinkError = fmt.Errorf(\"synthetic initialization failure\")")
		if control == "dependency" {
			links = replace(links, "func InitializationError() error { return firmlinkError }", "func InitializationError() error { return nil }")
		}
	}
	runner = replace(runner, "func run(args []string) int {", "func run(args []string) int {\nlogProcess(\"runner\", os.Getpid())\n")
	if fault == "late-start" {
		runner = replace(runner, "\tself, e := os.Executable()", "\ttime.Sleep(1500*time.Millisecond)\n\tself, e := os.Executable()")
	}
	if fault == "startup-stall" {
		runner = replace(runner, "\tself, e := os.Executable()", "\ttime.Sleep(20*time.Second)\n\tself, e := os.Executable()")
	}
	if fault == "runner-stall" || fault == "early-failure" {
		next := "time.Sleep(20*time.Second)"
		if fault == "early-failure" {
			next = "return 7"
		}
		runner = replace(runner, "\te = child.Run()", "\te = child.Start()\nif e != nil { return 1 }\nwaitReady(\"checker-ready\")\n"+next)
	}
	if control == "drain" {
		runner = replace(runner, "child.Stdout = os.Stdout", "child.Stdout = io.Discard")
	}
	if control == "stderr-drain" {
		runner = replace(runner, "child.Stderr = os.Stdout", "child.Stderr = io.Discard")
	}
	if fault != "dependency-failure" {
		unused := []string{`"agentguard/native/core"`, `"path/filepath"`, `"strings"`}
		if control != "drain" && control != "stderr-drain" {
			unused = append(unused, `"io"`)
		}
		for _, name := range unused {
			runner = replace(runner, name, "")
		}
	}
	if control == "failclosed" {
		entry = replace(entry, "(*) fail 'guard failed'", "(*) exit 0")
	}
	if control == "deadline" {
		entry = replace(entry, "/bin/sleep 3;", "/bin/sleep 6;")
	}
	if control == "cleanup" {
		entry = replace(entry, `kill -KILL -- -"$pid" 2>/dev/null`, ": # ablated runner-group cleanup")
	}
	fixture := replace(faultSource, "FAULT_LITERAL", strconv.Quote(fault))
	return faultPatch{runner, entry, links, fixture}, failure
}

func LifecycleViolations(fault string, result LifecycleResult) []string {
	problems := []string{}
	if result.TimedOut || result.SpawnError != "" || result.WaitError != "" {
		problems = append(problems, "harness process did not complete normally")
	}
	if len(result.AliveAtReturn) > 0 {
		problems = append(problems, "descendants alive at return")
	}
	if len(result.AliveAfterPoll) > 0 {
		problems = append(problems, "descendants survived cleanup")
	}
	var runner *ProcessRecord
	for i := range result.Processes {
		if result.Processes[i].Role == "runner" {
			runner = &result.Processes[i]
		}
	}
	groups := runner != nil && runner.PID == runner.PGID
	for _, process := range result.Processes {
		groups = groups && runner != nil && process.PGID == runner.PID
	}
	if !groups {
		problems = append(problems, "runner/checker/descendant process groups differ")
	}
	switch fault {
	case "pipe", "late-start", "leftover":
		if result.Status != 0 || result.Stderr != "" {
			problems = append(problems, "successful checker failed")
		}
		if fault == "pipe" {
			var event struct {
				Event struct {
					Marker string `json:"marker"`
				} `json:"event"`
				Pipe bool `json:"pipe"`
			}
			if json.Unmarshal([]byte(result.Stdout), &event) != nil || event.Event.Marker != "synthetic input" || !event.Pipe {
				problems = append(problems, "event/pipe contract changed")
			}
		} else if result.Stdout != "" {
			problems = append(problems, "unexpected successful output")
		}
		if fault == "late-start" && (result.Milliseconds <= 1000 || result.Milliseconds >= 3500) {
			problems = append(problems, "late startup timing changed")
		}
	case "large-reason", "large-stderr":
		if result.Status != 2 || result.Stdout != "" || result.Stderr != strings.Repeat("x", 131072)+"\n" || result.ProducerBytes != 131073 {
			problems = append(problems, "large reason/status did not drain byte-exactly")
		}
	default:
		if result.Status != 2 || result.Stdout != "" || !strings.Contains(result.Stderr, "so this call is blocked") {
			problems = append(problems, "operational failure did not deny")
		}
		if (fault == "panic" || fault == "partial-panic") && (strings.Contains(result.Stderr, "synthetic checker panic") || strings.Contains(result.Stderr, "PARTIAL_RESULT_CANARY") || strings.Contains(result.Stderr, "goroutine ")) {
			problems = append(problems, "checker panic leaked partial output or stack")
		}
		if (fault == "slow-reason" || fault == "hang" || fault == "runner-stall" || fault == "startup-stall") && (result.Milliseconds <= 2500 || result.Milliseconds >= 3500) {
			problems = append(problems, "three-second total deadline changed")
		}
	}
	return problems
}

func executeFault(entry, home string, body []byte) (result LifecycleResult, err error) {
	result.Processes = []ProcessRecord{}
	result.AliveAtReturn = []ProcessRecord{}
	result.AliveAfterPoll = []ProcessRecord{}
	defer func() {
		for _, process := range result.Processes {
			if process.PGID > 0 {
				if e := killGroup(process.PGID); e != nil && !errors.Is(e, syscall.EPERM) {
					err = errors.Join(err, e)
				}
			}
			if process.PID > 0 {
				if e := syscall.Kill(process.PID, syscall.SIGKILL); e != nil && !errors.Is(e, syscall.ESRCH) {
					err = errors.Join(err, e)
				}
			}
		}
	}()
	process, runErr := runProcess([]string{entry, "--runtime", "codex"}, body, home, []string{"HOME=" + home, "PATH=/usr/bin:/bin"}, 8*time.Second, func(_ ProcessResult) error {
		file, e := os.Open(filepath.Join(home, "processes.jsonl"))
		if e != nil {
			return e
		}
		defer file.Close()
		decoder := json.NewDecoder(file)
		for {
			var record ProcessRecord
			if e = decoder.Decode(&record); e != nil {
				if errors.Is(e, io.EOF) {
					break
				}
				return e
			}
			if record.PID <= 0 || record.PGID <= 0 {
				return errors.New("invalid process identity")
			}
			result.Processes = append(result.Processes, record)
		}
		living := func() ([]ProcessRecord, error) {
			rows := []ProcessRecord{}
			for _, record := range result.Processes {
				yes, e := alive(record.PID)
				if e != nil {
					return nil, e
				}
				if yes {
					rows = append(rows, record)
				}
			}
			return rows, nil
		}
		result.AliveAtReturn, e = living()
		if e != nil {
			return e
		}
		result.AliveAfterPoll = result.AliveAtReturn
		deadline := time.Now().Add(500 * time.Millisecond)
		for len(result.AliveAfterPoll) > 0 && time.Now().Before(deadline) {
			time.Sleep(25 * time.Millisecond)
			result.AliveAfterPoll, e = living()
			if e != nil {
				return e
			}
		}
		return nil
	})
	result.ProcessResult = process
	if runErr != nil {
		return result, runErr
	}
	if bytes, e := os.ReadFile(filepath.Join(home, "producer-bytes")); e == nil {
		result.ProducerBytes, err = strconv.Atoi(string(bytes))
	} else if !errors.Is(e, os.ErrNotExist) {
		err = e
	}
	return result, err
}

type LifecycleOptions struct{ Source, Output, Go, Control string }

func RunLifecycle(options LifecycleOptions) error {
	faults := LifecycleFaults
	if options.Control != "" {
		fault, ok := LifecycleControls[options.Control]
		if !ok {
			return errors.New("unknown lifecycle control")
		}
		faults = []string{fault}
	}
	output, err := newOutput(options.Output)
	if err != nil {
		return err
	}
	bindings, err := SourceBindings(options.Source)
	if err != nil {
		return err
	}
	build := filepath.Join(output, "test-build")
	for _, name := range []string{"native", "cmd", "go.mod", "go.sum"} {
		if err = copyTree(filepath.Join(options.Source, name), filepath.Join(build, name)); err != nil {
			return err
		}
	}
	read := func(name string) (string, error) {
		data, e := os.ReadFile(filepath.Join(options.Source, name))
		return string(data), e
	}
	original, err := read("cmd/agent-guard/main.go")
	if err != nil {
		return err
	}
	entry, err := read("bin/agent-guard")
	if err != nil {
		return err
	}
	links, err := read("native/filesystem/links.go")
	if err != nil {
		return err
	}
	env := []string{"PATH=/usr/bin:/bin", "HOME=" + build, "TMPDIR=" + output, "GOPROXY=off", "GOMODCACHE=" + os.Getenv("GOMODCACHE"), "GOCACHE=" + os.Getenv("GOCACHE")}
	file, err := os.OpenFile(filepath.Join(output, "results.jsonl"), os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		return err
	}
	defer file.Close()
	count, failed := 0, 0
	for _, fault := range faults {
		patch, err := injectFault(original, entry, links, fault, options.Control)
		if err != nil {
			return err
		}
		for name, text := range map[string]string{"cmd/agent-guard/main.go": patch.Runner, "cmd/agent-guard/fault.go": patch.Fixture, "native/filesystem/links.go": patch.Links} {
			if err = os.WriteFile(filepath.Join(build, name), []byte(text), 0600); err != nil {
				return err
			}
		}
		binary := filepath.Join(output, "fault-"+fault)
		compiled, err := runProcess([]string{options.Go, "build", "-trimpath", "-o", binary, "./cmd/agent-guard"}, nil, build, env, 60*time.Second, nil)
		if err != nil {
			return err
		}
		if err = writeJSON(filepath.Join(output, fault+"-build.json"), compiled); err != nil {
			return err
		}
		if compiled.Status != 0 || compiled.TimedOut {
			return fmt.Errorf("fault build failed: %s %s", compiled.SpawnError, compiled.Stderr)
		}
		binding, err := hashFile(binary, "agent-guard-native")
		if err != nil {
			return err
		}
		if fault == "large-reason" || fault == "large-stderr" {
			home, err := os.MkdirTemp(output, "producer-control-")
			if err != nil {
				return err
			}
			producer, err := runProcess([]string{binary, "--checker"}, []byte("{}"), home, []string{"HOME=" + home, "PATH=/usr/bin:/bin"}, 8*time.Second, nil)
			if err != nil {
				return err
			}
			payload := producer.Stdout
			if fault == "large-stderr" {
				payload = producer.Stderr
			}
			written, err := os.ReadFile(filepath.Join(home, "producer-bytes"))
			if err != nil {
				return err
			}
			if err = writeJSON(filepath.Join(output, fault+"-producer.json"), producer); err != nil {
				return err
			}
			if producer.Status != 2 || payload != strings.Repeat("x", 131072)+"\n" || string(written) != "131073" {
				return errors.New("independent producer control failed")
			}
		}
		for repeat := 0; repeat < 3; repeat++ {
			home, err := os.MkdirTemp(output, fmt.Sprintf("%s-%d-", fault, repeat))
			if err != nil {
				return err
			}
			bin := filepath.Join(home, "package/bin")
			if err = os.MkdirAll(bin, 0700); err != nil {
				return err
			}
			if err = copyFile(binary, filepath.Join(bin, "agent-guard-native")); err != nil {
				return err
			}
			if err = os.WriteFile(filepath.Join(bin, "agent-guard"), []byte(patch.Entry), 0700); err != nil {
				return err
			}
			body := []byte(`{"marker":"synthetic input"}`)
			if fault == "dependency-failure" {
				body = []byte(`{"tool_name":"WebFetch","tool_input":{}}`)
			}
			result, err := executeFault(filepath.Join(bin, "agent-guard"), home, body)
			if err != nil {
				return err
			}
			violations := LifecycleViolations(fault, result)
			count++
			if len(violations) > 0 {
				failed++
			}
			row := struct {
				LifecycleResult
				Fault      string   `json:"fault"`
				Repeat     int      `json:"repeat"`
				Violations []string `json:"violations"`
				Binary     Binding  `json:"binary"`
			}{result, fault, repeat, violations, binding}
			if err = json.NewEncoder(file).Encode(row); err != nil {
				return err
			}
			if err = file.Sync(); err != nil {
				return err
			}
			fmt.Printf("%s %d status=%d ms=%.1f violations=%v\n", fault, repeat, result.Status, result.Milliseconds, violations)
		}
	}
	if err = writeJSON(filepath.Join(output, "summary.json"), map[string]any{"cases": count, "failed_cases": failed, "control": options.Control, "source": bindings}); err != nil {
		return err
	}
	if failed > 0 {
		return fmt.Errorf("%d lifecycle cases violated their contract", failed)
	}
	return nil
}
