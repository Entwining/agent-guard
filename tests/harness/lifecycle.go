package harness

import (
	_ "embed"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"
	"time"
)

//go:embed testdata/fault.rs.txt
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
	runner := replace(original, "use crate::{", "mod lifecycle_fixture;\n\nuse crate::{")
	runner = replace(runner, "    let deadline = Instant::now() + CHECKER_TIMEOUT;", "    if let Some(status) = lifecycle_fixture::check(input, output, error) {\n        return status;\n    }\n    let deadline = Instant::now() + CHECKER_TIMEOUT;")
	if fault == "dependency-failure" {
		value := "Err(std::io::Error::other(\"synthetic initialization failure\"))"
		if control == "dependency" {
			value = "Ok(String::new())"
		}
		links = replace(links, "std::fs::read_to_string(\"/usr/share/firmlinks\")", value)
	}
	runner = replace(runner, "pub fn run(args: &[String]) -> io::Result<i32> {", "pub fn run(args: &[String]) -> io::Result<i32> {\n    lifecycle_fixture::log_process(\"runner\", std::process::id());")
	runner = replace(runner, "pub fn main(args: &[String]) -> i32 {", "pub fn main(args: &[String]) -> i32 {\n    if args.first().is_some_and(|arg| arg == \"--test-descendant\") {\n        return lifecycle_fixture::descendant();\n    }")
	if fault == "late-start" {
		runner = replace(runner, "    let mut child = Command::new(std::env::current_exe()?);", "    std::thread::sleep(Duration::from_millis(1500));\n    let mut child = Command::new(std::env::current_exe()?);")
	}
	if fault == "startup-stall" {
		runner = replace(runner, "    let mut child = Command::new(std::env::current_exe()?);", "    std::thread::sleep(Duration::from_secs(20));\n    let mut child = Command::new(std::env::current_exe()?);")
	}
	if fault == "runner-stall" || fault == "early-failure" {
		next := "std::thread::sleep(Duration::from_secs(20));\n    child.wait().map(status_code)"
		if fault == "early-failure" {
			next = "Ok(7)"
		}
		runner = replace(runner, "    supervise(&mut child)", "    let mut child = child.spawn()?;\n    lifecycle_fixture::wait_ready(\"checker-ready\");\n    "+next)
	}
	if control == "drain" {
		runner = replace(runner, ".stdout(Stdio::inherit())", ".stdout(Stdio::null())")
	}
	if control == "stderr-drain" {
		runner = replace(runner, ".stderr(Stdio::from(stdout))", ".stderr(Stdio::null())")
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
		if (fault == "panic" || fault == "partial-panic") && (strings.Contains(result.Stderr, "synthetic checker panic") || strings.Contains(result.Stderr, "PARTIAL_RESULT_CANARY") || strings.Contains(result.Stderr, "panicked at") || strings.Contains(result.Stderr, "stack backtrace:")) {
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

type LifecycleOptions struct{ Source, Output, Cargo, Control string }

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
	for _, name := range []string{"src", "tests", "examples", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "VERSION"} {
		if err = copyTree(filepath.Join(options.Source, name), filepath.Join(build, name)); err != nil {
			return err
		}
	}
	read := func(name string) (string, error) {
		data, e := os.ReadFile(filepath.Join(options.Source, name))
		return string(data), e
	}
	original, err := read("src/entry.rs")
	if err != nil {
		return err
	}
	entry, err := read("bin/agent-guard")
	if err != nil {
		return err
	}
	links, err := read("src/filesystem/links.rs")
	if err != nil {
		return err
	}
	cargo, err := exec.LookPath(options.Cargo)
	if err != nil {
		return err
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return err
	}
	cache := func(name, directory string) string {
		if value := os.Getenv(name); value != "" {
			return value
		}
		return filepath.Join(home, directory)
	}
	target := filepath.Join(output, "cargo-target")
	env := []string{"PATH=" + filepath.Dir(cargo) + ":/usr/bin:/bin", "HOME=" + build, "TMPDIR=" + output, "CARGO_NET_OFFLINE=true", "CARGO_HOME=" + cache("CARGO_HOME", ".cargo"), "RUSTUP_HOME=" + cache("RUSTUP_HOME", ".rustup"), "CARGO_TARGET_DIR=" + target}
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
		for name, text := range map[string]string{"src/entry.rs": patch.Runner, "src/entry/lifecycle_fixture.rs": patch.Fixture, "src/filesystem/links.rs": patch.Links} {
			if err = os.MkdirAll(filepath.Dir(filepath.Join(build, name)), 0700); err != nil {
				return err
			}
			if err = os.WriteFile(filepath.Join(build, name), []byte(text), 0600); err != nil {
				return err
			}
		}
		binary := filepath.Join(output, "fault-"+fault)
		compiled, err := runProcess([]string{cargo, "build", "--locked", "--release", "--bin", "agent-guard-native"}, nil, build, env, 90*time.Second, nil)
		if err != nil {
			return err
		}
		if err = writeJSON(filepath.Join(output, fault+"-build.json"), compiled); err != nil {
			return err
		}
		if compiled.Status != 0 || compiled.TimedOut {
			return fmt.Errorf("fault build failed: %s %s", compiled.SpawnError, compiled.Stderr)
		}
		if err = copyFile(filepath.Join(target, "release/agent-guard-native"), binary); err != nil {
			return err
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
				body = []byte(`{"tool_name":"Bash","tool_input":{"command":"true"}}`)
			}
			result, err := executeFault(filepath.Join(bin, "agent-guard"), home, body)
			if err != nil {
				logErr := writeJSON(filepath.Join(output, fmt.Sprintf("%s-%d-error.json", fault, repeat)), result)
				return errors.Join(err, logErr)
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
