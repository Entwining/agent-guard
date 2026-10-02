package filesystem

import (
	"os"
	"path"
	"strings"
	"syscall"
)

func ExpandHome(p, home string) string {
	// Tilde-user matching follows USER, including an empty value; account lookup would change that contract.
	username, present := os.LookupEnv("USER")
	if !present {
		username = "unknown"
	}
	for _, prefix := range []string{"~", "~" + username} {
		if p == prefix || strings.HasPrefix(p, prefix+"/") {
			return home + p[len(prefix):]
		}
	}
	return p
}

func Unfirmlink(p string) string {
	prefix := "/system/volumes/data"
	l := strings.ToLower(p)
	if l == prefix {
		return "/"
	}
	if strings.HasPrefix(l, prefix+"/") {
		return p[len(prefix):]
	}
	return p
}

func StripFileURL(p string) string {
	if strings.HasPrefix(strings.ToLower(p), "file://") {
		return p[7:]
	}
	return p
}

func Resolve(cwd, p string) string {
	if strings.HasPrefix(p, "/") {
		return path.Clean(p)
	}
	if !strings.HasPrefix(cwd, "/") {
		// os.Getwd checks PWD with stat; only the process's actual directory is needed here.
		dir, e := syscall.Getwd()
		if e != nil {
			panic(e)
		}
		cwd = dir + "/" + cwd
	}
	return path.Clean(cwd + "/" + p)
}

func AbsPath(p, cwd, home string, quoted ...bool) string {
	if len(quoted) == 0 || !quoted[0] {
		p = ExpandHome(p, home)
	}
	return Unfirmlink(Resolve(cwd, StripFileURL(p)))
}
