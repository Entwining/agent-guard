package harness

import (
	"strings"
	"testing"
)

func TestLifecycleRejectsRustPanicLeak(t *testing.T) {
	for _, fault := range []string{"panic", "partial-panic"} {
		for _, trace := range []string{"", "thread 'main' panicked at fixture.rs:1", "stack backtrace:"} {
			result := LifecycleResult{
				ProcessResult: ProcessResult{Status: 2, Stderr: "agent-guard failed, so this call is blocked\n" + trace},
				Processes:     []ProcessRecord{{Role: "runner", PID: 100, PGID: 100}},
			}
			violations := LifecycleViolations(fault, result)
			leak := strings.Contains(strings.Join(violations, "\n"), "checker panic leaked")
			if leak != (trace != "") || (trace == "" && len(violations) != 0) {
				t.Fatalf("%s trace=%q: %v", fault, trace, violations)
			}
		}
	}
}
