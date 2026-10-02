package rules

import (
	"regexp"
	"strings"

	"agentguard/native/filesystem"
	"agentguard/native/reasons"
	"agentguard/native/record"
)

func rx(p, s string) bool { return regexp.MustCompile(p).MatchString(s) }

func groups(ts []record.Target) (map[int][]record.Target, []int) {
	m := map[int][]record.Target{}
	order := []int{}
	for _, t := range ts {
		if _, ok := m[t.Command]; !ok {
			order = append(order, t.Command)
		}
		m[t.Command] = append(m[t.Command], t)
	}
	return m, order
}

func Appdata(req record.Request, ts []record.Target) []string {
	filtered := []record.Target{}
	for _, t := range ts {
		if t.Via != "items" && !(t.Via == "tool" && t.Glob) {
			filtered = append(filtered, t)
		}
	}
	m, order := groups(filtered)
	denials := []string{}
	trees := strings.Join(filesystem.AppdataTrees, "|")
	for _, i := range order {
		app, broad := false, false
		for _, t := range m[i] {
			touches := t.Effect != "name" || t.Glob
			if touches && t.Via != "scan" {
				app = app || t.Expands && rx(`(?is)/Library/(`+trees+`)(/.*)?$`, t.Path) || filesystem.IsAppdata(t.Path, req.Home, t.Glob)
			}
			switch {
			case t.Via == "scan":
				broad = broad || filesystem.IsBroad(t.Path, req.Home) || filesystem.IsAppdata(t.Path, req.Home)
			case !touches:
			case t.Via == "tool":
				broad = broad || t.Search && filesystem.IsLibrary(t.Path, req.Home)
			default:
				broad = broad || filesystem.IsBroad(t.Path, req.Home, t.Glob) && (t.Walk != "none" || t.Glob)
			}
		}
		if app {
			denials = append(denials, reasons.Appdata)
		} else if broad {
			denials = append(denials, reasons.Broad)
		}
	}
	sig := `(?i)(~|\$HOME|\$\{HOME\}|` + regexp.QuoteMeta(req.Home) + `)/Library/(` + trees + `)`
	for _, f := range req.Uninspectable {
		if rx(sig, f.Text) {
			denials = append(denials, reasons.Appdata)
		}
	}
	return denials
}
