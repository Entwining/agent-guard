package filesystem

import "testing"

// This seam exists only in the test package, never in either production executable.
func InjectInitializationFailure(t *testing.T, err error) {
	t.Helper()
	previous := firmlinkError
	firmlinkError = err
	t.Cleanup(func() { firmlinkError = previous })
}
