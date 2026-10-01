package filesystem

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestLinkBudget(t *testing.T) {
	// Ancestor aliases must not change the expected path of the synthetic chain.
	home, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	for _, chain := range []int{8, 9} {
		start := home + "/chain" + strings.Repeat("x", chain)
		for i := 0; i < chain; i++ {
			current := start + strings.Repeat("a", i)
			next := start + strings.Repeat("a", i+1)
			if err := os.Symlink(next, current); err != nil {
				t.Fatal(err)
			}
		}
		result, err := FollowLinks(t.Context(), start, home, DiskProbe{}, nil)
		if chain == 8 && (err != nil || result != start+strings.Repeat("a", chain)) {
			t.Fatalf("ordinary chain: %s %v", result, err)
		}
		if chain == 9 && err == nil {
			t.Fatal("over-budget chain allowed")
		}
	}
}
