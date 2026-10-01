package shell

import (
	"agentguard/native/filesystem"
	"agentguard/native/record"
	"agentguard/native/targets"
	"slices"
	"strings"
)

func commandSources(cmd *record.Command, home string) ([]child, []string) {
	children := []child{}
	code := []string{}
	name := record.ProgramName(record.Text(cmd.Argv, cmd.Program))
	rest := record.Rest(cmd)
	switch {
	case slices.Contains([]string{"rg", "grep", "ag", "ack"}, name):
		targets.SearchRoles(cmd, rest, name)
	case name == "find":
		for n, word := range rest {
			if !slices.Contains([]string{"-exec", "-execdir", "-ok", "-okdir"}, word.Text) {
				continue
			}
			end := len(rest)
			for k := n + 1; k < len(rest); k++ {
				if rest[k].Text == ";" || rest[k].Text == "+" {
					end = k
					break
				}
			}
			roots := targets.FindRoots(rest)
			root := record.Text(roots, 0)
			if root == "" {
				root = "."
			}
			children = append(children, child{source: rawJoin(rest[n+1 : end]), items: &record.Items{Root: filesystem.AbsPath(root, cmd.Cwd, home), Hidden: true}})
		}
	case name == "fd":
		pattern := true
		roots := []string{}
		for n := 0; n < len(rest); n++ {
			arg := rest[n].Text
			if slices.Contains([]string{"-x", "-X", "--exec", "--exec-batch"}, arg) {
				rest[n].Role = "option"
				root := "."
				if len(roots) > 0 {
					root = roots[0]
				}
				children = append(children, child{source: rawJoin(rest[n+1:]), items: &record.Items{Root: filesystem.AbsPath(root, cmd.Cwd, home), Hidden: targets.ShowsHidden(rest)}})
				break
			} else if (rx(`^(--search-path|--base-directory)(=|$)`, arg) || arg == "-C") && (strings.Contains(arg, "=") || n+1 < len(rest)) {
				sep := !strings.Contains(arg, "=")
				base := arg == "-C" || strings.HasPrefix(arg, "--base-directory")
				if sep {
					rest[n].Role = "option"
					n++
				}
				p := rest[n]
				p.Role = "path"
				if sep {
					p.Value = p.Text
				} else {
					p.Value = arg[strings.Index(arg, "=")+1:]
				}
				if base {
					p.Value = filesystem.ExpandHome(p.Value, home)
					cmd.Cwd = changeDirectory(cmd.Cwd, p.Value, !p.Expands && !p.Globs)
				}
			} else if slices.Contains(strings.Fields("-E -e -t -d --exclude --extension --type --max-depth"), arg) {
				rest[n].Role = "option"
				if n+1 < len(rest) {
					n++
					rest[n].Role = "optarg"
				}
			} else if strings.HasPrefix(arg, "-") {
				rest[n].Role = "option"
			} else if pattern {
				rest[n].Role = "pattern"
				pattern = false
			} else {
				rest[n].Role = "path"
				roots = append(roots, arg)
			}
		}
	case name == "du":
		for n := 0; n < len(rest); n++ {
			arg := rest[n].Text
			if slices.Contains(strings.Fields("-d -I -B -t --max-depth --exclude --block-size --threshold"), arg) || rx(`^-[^-]+[dIBt]$`, arg) {
				rest[n].Role = "option"
				if n+1 < len(rest) {
					n++
					rest[n].Role = "optarg"
				}
			} else if strings.HasPrefix(arg, "-") {
				rest[n].Role = "option"
			}
		}
	case slices.Contains(shellPrograms, name):
		for n, a := range rest {
			if rx(shellCodeFlag, a.Text) {
				if n+1 < len(rest) {
					rest[n+1].Role = "code"
				}
				children = append(children, child{source: record.Text(rest, n+1)})
				break
			}
		}
	case name == "eval" && (cmd.Shell || slices.Contains(cmd.Wrappers, "command")):
		ss := []string{}
		for _, a := range rest {
			ss = append(ss, a.Text)
		}
		children = append(children, child{source: strings.Join(ss, " ")})
	case interpreterName(name) != "":
		code = append(code, interpreterCode(interpreterName(name), rest)...)
	}
	return children, code
}
