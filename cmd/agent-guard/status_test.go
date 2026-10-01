package main

import "testing"

func TestCheckerStatus(t *testing.T) {
	for _, row := range []struct {
		name            string
		status, encoded int
	}{
		{"allow", 0, 0},
		{"operational-error", 1, 1},
		{"completed-denial", 2, checkerDenial},
	} {
		t.Run(row.name, func(t *testing.T) {
			if got := checkerStatus(row.status); got != row.encoded {
				t.Fatalf("checker status %d became %d, want %d", row.status, got, row.encoded)
			}
			if row.status == 2 && (row.encoded == 0 || row.encoded == 2) {
				t.Fatal("completed denial collides with allow or Go panic")
			}
		})
	}
}
