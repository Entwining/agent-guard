package filesystem

import (
	"context"
	"sync"

	"agentguard/native/record"
)

// Bounded parallel resolution improved existing multi-target workloads.
const targetConcurrency = 8

func LinkedTargets(ctx context.Context, ts []record.Target, home string, probe Probe) ([]record.Target, error) {
	if e := ctx.Err(); e != nil {
		return nil, e
	}
	result := append([]record.Target{}, ts...)
	errs := make([]error, len(ts))
	candidates := make([]int, 0, len(ts))
	for i, t := range ts {
		if t.Effect == "name" && !t.Glob || t.Via == "tool" && t.Glob {
			continue
		}
		candidates = append(candidates, i)
	}
	resolve := func(i int) {
		t := ts[i]
		var p string
		if t.Glob || t.Expands {
			p, errs[i] = followPrefix(ctx, t.Unresolved, home, probe)
		} else {
			p, errs[i] = FollowLinks(ctx, t.Unresolved, home, probe, nil)
		}
		if errs[i] == nil && p != t.Unresolved {
			result[i].Path = p
			result[i].Unresolved = p
		}
	}
	workers := min(targetConcurrency, len(candidates))
	if workers < 2 {
		for _, i := range candidates {
			resolve(i)
		}
	} else {
		var joined sync.WaitGroup
		joined.Add(workers)
		for worker := 0; worker < workers; worker++ {
			go func(worker int) {
				defer joined.Done()
				for index := worker; index < len(candidates); index += workers {
					if ctx.Err() != nil {
						return
					}
					resolve(candidates[index])
				}
			}(worker)
		}
		joined.Wait()
	}
	if e := ctx.Err(); e != nil {
		return nil, e
	}
	changed := false
	for i, e := range errs {
		if e != nil {
			return nil, e
		}
		changed = changed || result[i].Unresolved != ts[i].Unresolved
	}
	if !changed {
		return nil, nil
	}
	return result, nil
}
