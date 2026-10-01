package filesystem

import (
	"errors"
	"os"
	"syscall"
)

// Callers may share a Probe across target resolutions. Both methods must be safe
// for concurrent calls; mutable observers own their synchronization and join before reading results.
type Probe interface {
	Readlink(string) (string, error)
	Stat(string) (os.FileInfo, error)
}
type DiskProbe struct{}

func (DiskProbe) Readlink(p string) (string, error) { return os.Readlink(p) }

func (DiskProbe) Stat(p string) (os.FileInfo, error) { return os.Stat(p) }

func NotALink(e error) bool {
	return errors.Is(e, syscall.EINVAL) || errors.Is(e, syscall.ENOENT) || errors.Is(e, syscall.ENOTDIR) || errors.Is(e, syscall.ENAMETOOLONG)
}
