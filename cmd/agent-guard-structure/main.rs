#![deny(clippy::unwrap_used, clippy::expect_used)]
#![forbid(unsafe_code)]

use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

const MAX_EFFECTIVE_LINES: usize = 500;

fn main() -> ExitCode {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let [root] = args.as_slice() else {
        eprintln!("usage: agent-guard-structure repository-root");
        return ExitCode::FAILURE;
    };
    match oversized_files(Path::new(root)) {
        Ok(files) if files.is_empty() => ExitCode::SUCCESS,
        Ok(files) => {
            for (path, count) in files {
                eprintln!(
                    "{}: {count} effective lines exceeds {MAX_EFFECTIVE_LINES}",
                    path.display()
                );
            }
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn oversized_files(root: &Path) -> Result<Vec<(PathBuf, usize)>, String> {
    let mut pending = ["src", "tests", "cmd", "examples"]
        .map(PathBuf::from)
        .to_vec();
    let mut oversized = Vec::new();
    while let Some(relative) = pending.pop() {
        let directory = root.join(&relative);
        let entries = fs::read_dir(&directory)
            .map_err(|error| format!("{}: {error}", directory.display()))?;
        for entry in entries {
            let entry = entry.map_err(|error| format!("{}: {error}", directory.display()))?;
            let path = relative.join(entry.file_name());
            let kind = entry
                .file_type()
                .map_err(|error| format!("{}: {error}", path.display()))?;
            // Following source links could leave the four owned source roots.
            if kind.is_symlink() {
                return Err(format!(
                    "{}: source symlink cannot be checked",
                    path.display()
                ));
            }
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() && path.extension().is_some_and(|extension| extension == "rs")
            {
                let source = fs::read_to_string(root.join(&path))
                    .map_err(|error| format!("{}: {error}", path.display()))?;
                let count = source
                    .lines()
                    .filter(|line| {
                        let line = line.trim();
                        !line.is_empty() && !line.starts_with("//")
                    })
                    .count();
                if count > MAX_EFFECTIVE_LINES {
                    oversized.push((path, count));
                }
            }
        }
    }
    oversized.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(oversized)
}
