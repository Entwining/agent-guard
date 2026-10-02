package filesystem

import (
	"context"
	"errors"
	"fmt"
	"path"
	"reflect"
	"strings"
	"sync/atomic"
	"syscall"
	"testing"
	"time"

	"agentguard/native/record"
)

type targetProbe struct {
	DiskProbe
	readlink func(string) (string, error)
}

func (p targetProbe) Readlink(path string) (string, error) { return p.readlink(path) }
func targetList(home string, count int) []record.Target {
	ts := make([]record.Target, count)
	for i := range ts {
		p := fmt.Sprintf("%s/link%d", home, i)
		ts[i] = record.Target{Path: p, Unresolved: p, Effect: "read"}
	}
	return ts
}
func TestTargetConcurrency(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 2*time.Second)
	defer cancel()
	home := t.TempDir()
	ts := targetList(home, 19)
	var active, peak atomic.Int64
	started := make(chan string, len(ts))
	release := make(chan struct{})
	p := targetProbe{readlink: func(p string) (string, error) {
		if !strings.HasPrefix(path.Base(p), "link") {
			return "", syscall.EINVAL
		}
		current := active.Add(1)
		defer active.Add(-1)
		for old := peak.Load(); current > old; old = peak.Load() {
			if peak.CompareAndSwap(old, current) {
				break
			}
		}
		started <- p
		select {
		case <-release:
			return "", syscall.EINVAL
		case <-ctx.Done():
			return "", ctx.Err()
		}
	}}
	done := make(chan error, 1)
	go func() { defer close(done); _, e := LinkedTargets(ctx, ts, home, p); done <- e }()
	defer func() {
		cancel()
		for range done {
		}
	}()
	for i := 0; i < 8; i++ {
		select {
		case <-started:
		case <-ctx.Done():
			t.Fatal("bounded target work did not overlap")
		}
	}
	select {
	case p := <-started:
		t.Errorf("ninth resolution started before a worker finished: %s", p)
	case <-time.After(25 * time.Millisecond):
	}
	close(release)
	select {
	case e := <-done:
		if e != nil {
			t.Fatal(e)
		}
	case <-ctx.Done():
		t.Fatal("resolution did not finish")
	}
	if peak.Load() > 8 || active.Load() != 0 {
		t.Fatalf("peak %d, active after return %d", peak.Load(), active.Load())
	}
}
func TestTargetOrder(t *testing.T) {
	home := t.TempDir()
	ts := targetList(home, 2)
	t.Run("results", func(t *testing.T) {
		input := append(append([]record.Target{}, ts...), record.Target{Path: home + "/unchanged", Unresolved: home + "/plain/../unchanged", Effect: "read"})
		ctx, cancel := context.WithTimeout(t.Context(), time.Second)
		defer cancel()
		p := targetProbe{readlink: orderedProbe(ctx, ts, home, nil, nil)}
		got, e := LinkedTargets(ctx, input, home, p)
		if e != nil {
			t.Fatal(e)
		}
		want := append([]record.Target{}, input...)
		want[0].Path = home + "/first"
		want[0].Unresolved = want[0].Path
		want[1].Path = home + "/second"
		want[1].Unresolved = want[1].Path
		if !reflect.DeepEqual(got, want) || ts[0].Path != home+"/link0" {
			t.Fatalf("result order or input ownership changed: %+v", got)
		}
	})
	t.Run("errors", func(t *testing.T) {
		ctx, cancel := context.WithTimeout(t.Context(), time.Second)
		defer cancel()
		first, second := errors.New("first target error"), errors.New("second target error")
		p := targetProbe{readlink: orderedProbe(ctx, ts, home, first, second)}
		got, e := LinkedTargets(ctx, ts, home, p)
		if got != nil || !errors.Is(e, first) {
			t.Fatalf("completion order chose error: %+v %v", got, e)
		}
	})
}

func TestLinkCancellation(t *testing.T) {
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	var calls atomic.Int64
	p := targetProbe{readlink: func(string) (string, error) { calls.Add(1); cancel(); return "", syscall.EINVAL }}
	home := t.TempDir()
	got, e := FollowLinks(ctx, home+"/nested/file", home, p, nil)
	if got != "" || !errors.Is(e, context.Canceled) || calls.Load() != 1 {
		t.Fatalf("canceled traversal continued: %q %v, %d probes", got, e, calls.Load())
	}
}
func orderedProbe(ctx context.Context, ts []record.Target, home string, first, second error) func(string) (string, error) {
	later := make(chan struct{})
	return func(p string) (string, error) {
		switch p {
		case ts[0].Unresolved:
			select {
			case <-later:
			case <-ctx.Done():
				return "", ctx.Err()
			}
			return home + "/first", first
		case ts[1].Unresolved:
			close(later)
			return home + "/second", second
		default:
			return "", syscall.EINVAL
		}
	}
}
func TestTargetCancellation(t *testing.T) {
	ctx, cancel := context.WithCancel(t.Context())
	home := t.TempDir()
	ts := targetList(home, 19)
	started := make(chan struct{}, 8)
	var calls, active atomic.Int64
	p := targetProbe{readlink: func(p string) (string, error) {
		if !strings.HasPrefix(path.Base(p), "link") {
			return "", syscall.EINVAL
		}
		calls.Add(1)
		active.Add(1)
		defer active.Add(-1)
		started <- struct{}{}
		<-ctx.Done()
		return "", ctx.Err()
	}}
	done := make(chan error, 1)
	go func() { defer close(done); _, e := LinkedTargets(ctx, ts, home, p); done <- e }()
	defer func() {
		cancel()
		for range done {
		}
	}()
	select {
	case <-started:
	case <-time.After(time.Second):
		t.Fatal("work never reached probe")
	}
	cancel()
	select {
	case e := <-done:
		if !errors.Is(e, context.Canceled) {
			t.Fatal(e)
		}
	case <-time.After(time.Second):
		t.Fatal("canceled workers did not join")
	}
	if active.Load() != 0 || calls.Load() > 8 {
		t.Fatalf("canceled work leaked: active %d, calls %d", active.Load(), calls.Load())
	}
	before := calls.Load()
	got, e := LinkedTargets(ctx, ts, home, p)
	if got != nil || !errors.Is(e, context.Canceled) || calls.Load() != before {
		t.Fatal("already canceled request probed or succeeded")
	}
}
