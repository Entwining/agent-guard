package main

import (
	"flag"
	"fmt"
	"os"

	"agentguard/tests/harness"
)

func main() {
	source := flag.String("source", ".", "source checkout; faults run only in an external copy")
	output := flag.String("output", "", "new evidence directory outside Git checkouts")
	cargo := flag.String("cargo", "cargo", "Cargo executable for copied Rust fault builds")
	control := flag.String("control", "", "negative control: drain, stderr-drain, failclosed, deadline, cleanup, dependency")
	flag.Parse()
	if err := harness.RunLifecycle(harness.LifecycleOptions{Source: *source, Output: *output, Cargo: *cargo, Control: *control}); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
