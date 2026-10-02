package main

import (
	"flag"
	"fmt"
	"os"
	"strings"

	"agentguard/tests/harness"
)

func main() {
	hook := flag.String("hook", "", "run the synthetic hook adapter for claude, pi, or codex")
	entry := flag.String("entry", "", "absolute assembled bin/agent-guard path")
	output := flag.String("output", "", "new evidence directory outside Git checkouts")
	source := flag.String("source", ".", "source checkout for the evidence manifest")
	runtimes := flag.String("runtimes", "claude,pi,codex", "comma-separated runtimes; each runs all ten cases three times")
	ablate := flag.Bool("ablate", false, "synthetic no-guard control; every case must execute")
	flag.Parse()
	if *hook != "" {
		os.Exit(harness.HookMain(*hook, os.Stdin, os.Stdout, os.Stderr))
	}
	helper, err := os.Executable()
	if err == nil {
		err = harness.RunRuntime(harness.RuntimeOptions{Source: *source, Entry: *entry, Output: *output, Helper: helper, Runtimes: strings.Split(*runtimes, ","), Ablate: *ablate})
	}
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
