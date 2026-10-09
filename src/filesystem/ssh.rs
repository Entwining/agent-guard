use super::{CheckError, CheckErrorKind, Metadata, Probe, lexical};
use std::path::Path;

pub(super) fn near(candidate: &str, root: &str, search: bool) -> bool {
    let candidate = candidate.trim_end_matches('/').to_lowercase();
    let candidate = if candidate.is_empty() {
        "/"
    } else {
        &candidate
    };
    let root = root.to_lowercase();
    candidate == root
        || candidate.starts_with(&format!("{root}/"))
        || search && (candidate == "/" || root.starts_with(&format!("{candidate}/")))
}

pub(super) fn checked_stat(
    path: &str,
    home: &str,
    resolved_home: &str,
    probe: &mut dyn Probe,
    lexical: &mut lexical::Lexical,
) -> Result<Option<Metadata>, CheckError> {
    // The caller has already decided protected spellings; keep that stop at the
    // probe boundary too, including aliases under a resolved HOME.
    if lexical
        .check_both(path, home, Some(resolved_home), false)?
        .is_some()
    {
        return Err(CheckError {
            kind: CheckErrorKind::ProbeFault,
        });
    }
    probe.stat(Path::new(path)).map_err(|_| CheckError {
        kind: CheckErrorKind::ProbeFault,
    })
}

pub(super) fn same_file(
    a: &str,
    b: &str,
    home: &str,
    resolved_home: &str,
    probe: &mut dyn Probe,
    lexical: &mut lexical::Lexical,
) -> Result<bool, CheckError> {
    let x = checked_stat(a, home, resolved_home, probe, lexical)?;
    let y = checked_stat(b, home, resolved_home, probe, lexical)?;
    Ok(matches!((x, y), (Some(x), Some(y)) if (x.device, x.inode) == (y.device, y.inode)))
}

pub(super) fn ssh_public(name: &str) -> bool {
    name == "config"
        || name.starts_with("config.")
        || name.ends_with(".pub")
        || name == "allowed_signers"
        || name.starts_with("known_hosts")
}
