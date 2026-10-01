package shell

import "testing"

func TestPwdWhitespace(t *testing.T) {
	for _, separator := range []string{"\ufeff", "\u00a0", "\v", "\u0085", "\u200b"} {
		script := ParseScript(`cat "$(pwd`+separator+`)"`, "/synthetic/project", "/synthetic")
		last := script.Commands[len(script.Commands)-1]
		want := separator != "\u0085" && separator != "\u200b"
		if last.Argv[1].Pwd != want {
			t.Errorf("separator %U Pwd %t, want %t", []rune(separator)[0], last.Argv[1].Pwd, want)
		}
	}
}
