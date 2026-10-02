package rules

import (
	"reflect"
	"testing"

	"agentguard/native/reasons"
)

func TestSignatureWhitespace(t *testing.T) {
	for _, separator := range []string{"\ufeff", "\u00a0", "\u2003", "\v", "\u0085", "\u200b"} {
		space := separator != "\u0085" && separator != "\u200b"
		for _, c := range []struct{ fragment, reason string }{
			{"env" + separator, reasons.Dump},
			{"declare" + separator + "-p", reasons.Dump},
			{"curl" + separator + "x" + separator + "--verbose", reasons.Trace},
			{"gh" + separator + "auth" + separator + "token", reasons.Token},
			{"security" + separator + "x" + separator + "-w", reasons.Keychain},
			{"ECHO" + separator + "x $CANARY_TOKEN", reasons.Variable},
		} {
			t.Run(c.fragment, func(t *testing.T) {
				want := []string{}
				if space {
					want = append(want, c.reason)
				}
				if got := SecretSignatures(c.fragment); !reflect.DeepEqual(got, want) {
					t.Fatalf("reasons %q, want %q", got, want)
				}
			})
		}
	}
}
