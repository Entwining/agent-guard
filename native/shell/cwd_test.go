package shell

import (
	"agentguard/native/record"
	"fmt"
	"strings"
	"testing"
)

func TestCwdBudget(t *testing.T) {
	var input strings.Builder
	for i := 1; i <= 22; i++ {
		fmt.Fprintf(&input, "cd d%d; ", i)
	}
	input.WriteString("cat file.txt")
	script := ParseScript(input.String(), "/synthetic-home", "/synthetic-home")
	if script.ParseFailed {
		t.Fatal("ordinary cd sequence failed to parse")
	}
	counts := map[string]int{}
	for _, command := range script.Commands {
		key := record.Text(command.Argv, 0) + " " + record.Text(command.Argv, 1)
		counts[key]++
		if counts[key] > 17 {
			t.Fatalf("one statement produced %d cwd alternatives: %s", counts[key], key)
		}
	}
	if counts["cat file.txt"] == 0 {
		t.Fatal("terminal read disappeared")
	}
}
