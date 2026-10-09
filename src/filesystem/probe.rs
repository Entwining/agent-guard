use super::{FileKind, Metadata, Probe};
use std::{
    io,
    path::{Path, PathBuf},
};

pub struct DiskProbe;

impl Probe for DiskProbe {
    fn stat(&mut self, path: &Path) -> io::Result<Option<Metadata>> {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(path) {
            Ok(info) => Ok(Some(Metadata {
                device: info.dev(),
                inode: info.ino(),
                kind: if info.is_dir() {
                    FileKind::Directory
                } else if info.is_file() {
                    FileKind::File
                } else {
                    FileKind::Other
                },
            })),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
    fn read_link(&mut self, path: &Path) -> io::Result<Option<PathBuf>> {
        match std::fs::read_link(path) {
            Ok(target) => Ok(Some(target)),
            // Darwin ENAMETOOLONG is a benign non-link result.
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound
                        | io::ErrorKind::InvalidInput
                        | io::ErrorKind::NotADirectory
                ) || error.raw_os_error() == Some(63) =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
}

pub(crate) fn canonicalize_home(home: impl AsRef<Path>) -> io::Result<PathBuf> {
    std::fs::canonicalize(home)
}
