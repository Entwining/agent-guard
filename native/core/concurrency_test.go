package core

import (
	"context"
	"errors"
	"sync"
	"sync/atomic"
	"syscall"
	"testing"

	"agentguard/native/filesystem"
)

type cancelingProbe struct {
	filesystem.DiskProbe
	cancel context.CancelFunc
	calls  atomic.Int64
}

func (p *cancelingProbe) Readlink(string) (string, error) {
	p.calls.Add(1)
	p.cancel()
	return "", syscall.EINVAL
}

func TestInFlightCancellation(t *testing.T) {
	home := syntheticHome(t)
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	p := &cancelingProbe{cancel: cancel}
	req := BuildRequest("claude", "bash", home+"/project", "cat ordinary", "", home)
	reason, e := Evaluate(ctx, req, p)
	if reason != "" || !errors.Is(e, context.Canceled) || p.calls.Load() != 1 {
		t.Fatalf("in-flight cancellation: %q %v, %d probes", reason, e, p.calls.Load())
	}
}

func TestSharedProbeRequests(t *testing.T) {
	home := syntheticHome(t)
	req := BuildRequest("claude", "bash", home+"/project", "printf '%s\\n' a b | xargs cat", "", home)
	p := &watchingProbe{}
	var joined sync.WaitGroup
	for i := 0; i < 8; i++ {
		joined.Add(1)
		go func() {
			defer joined.Done()
			reason, e := Evaluate(t.Context(), req, p)
			if e != nil || reason != "" {
				t.Errorf("shared request: %q %v", reason, e)
			}
		}()
	}
	joined.Wait()
	if len(p.readlinks) == 0 || len(p.violations) != 0 {
		t.Fatalf("shared observer: %d probes, %v", len(p.readlinks), p.violations)
	}
}
func TestCanceledRequest(t *testing.T) {
	home := syntheticHome(t)
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	p := &watchingProbe{}
	req := BuildRequest("claude", "bash", home+"/project", "ls", "", home)
	reason, e := Evaluate(ctx, req, p)
	if reason != "" || !errors.Is(e, context.Canceled) || len(p.readlinks) != 0 || len(p.stats) != 0 {
		t.Fatalf("cancellation became a verdict: %q %v", reason, e)
	}
	result, e := CheckEvent(ctx, "claude", home+"/project", home, []byte(`{"tool_input":{"command":"ls"}}`))
	if !errors.Is(e, context.Canceled) || result.Exit != 0 || len(result.Stdout) != 0 || len(result.Stderr) != 0 {
		t.Fatalf("canceled protocol: %+v %v", result, e)
	}
}
