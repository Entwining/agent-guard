package main

import (
	"agentguard/tests/harness"
	"flag"
	"fmt"
	"os"
)

func main() {
	source := flag.String("source", ".", "source checkout; faults run only in an external copy")
	output := flag.String("output", "", "new evidence directory outside Git checkouts")
	goBinary := flag.String("go", "go", "Go executable")
	control := flag.String("control", "", "negative control: drain, stderr-drain, failclosed, deadline, cleanup, dependency")
	flag.Parse()
	if err := harness.RunLifecycle(harness.LifecycleOptions{Source: *source, Output: *output, Go: *goBinary, Control: *control}); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
