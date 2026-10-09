use super::{glob, lexical_literal};
use std::path::{Component, Path, PathBuf};

pub fn normalize(path: &str, cwd: &str, home: &str) -> String {
    let joined = absolute_input(path, cwd, home);
    let mut clean = PathBuf::new();
    for part in Path::new(&joined).components() {
        match part {
            Component::ParentDir => {
                clean.pop();
            }
            Component::CurDir => {}
            part => clean.push(part.as_os_str()),
        }
    }
    unfirmlink(strip_path_aliases(clean.to_str().unwrap_or("")))
}

pub(crate) fn absolute_input(path: &str, cwd: &str, home: &str) -> String {
    let expanded = if let Some(tail) = path.strip_prefix("~/") {
        format!("{home}/{tail}")
    } else if path == "~" {
        home.to_owned()
    } else {
        path.to_owned()
    };
    let expanded = strip_path_aliases(strip_file_url(&expanded));
    if expanded.starts_with('/') {
        expanded.to_owned()
    } else {
        format!("{cwd}/{expanded}")
    }
}

pub(crate) fn strip_path_aliases(mut path: &str) -> &str {
    loop {
        if let Some(rest) = path.strip_prefix("/.nofollow")
            && rest.starts_with('/')
        {
            path = rest;
        } else if let Some(rest) = path.strip_prefix("/.resolve/")
            && let Some((device, _)) = rest.split_once('/')
            && !device.is_empty()
            && device.bytes().all(|byte| byte.is_ascii_digit())
        {
            path = &rest[device.len()..];
        } else {
            return path;
        }
    }
}

pub(crate) fn literal_shell_pattern(text: &str) -> String {
    glob::shell_pattern(text, &std::iter::once(0..text.len()).collect::<Vec<_>>())
}

pub(crate) fn descriptor_path(path: &str) -> bool {
    let path = strip_path_aliases(path);
    path.strip_prefix("/dev/fd/").is_some_and(|tail| {
        let number = tail.split('/').next().unwrap_or("");
        !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
    })
}

pub(crate) fn absolute_pattern(path: &str, cwd: &str) -> String {
    if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("{}/{path}", literal_shell_pattern(cwd))
    }
}

pub(crate) fn strip_file_url(path: &str) -> &str {
    if path
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file://"))
    {
        &path[7..]
    } else {
        path
    }
}

pub(super) fn unfirmlink(path: &str) -> String {
    unfirmlink_path(path).to_owned()
}

pub(super) fn unfirmlink_path(path: &str) -> &str {
    let prefix = "/system/volumes/data";
    if path.eq_ignore_ascii_case(prefix) {
        "/"
    } else if path
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        && path.as_bytes().get(prefix.len()) == Some(&b'/')
    {
        &path[prefix.len()..]
    } else {
        path
    }
}

pub fn expand_home(path: &str, home: &str, user: Option<&str>) -> String {
    for prefix in ["~".to_owned(), format!("~{}", user.unwrap_or("unknown"))] {
        if path == prefix || path.starts_with(&format!("{prefix}/")) {
            return format!("{home}{}", &path[prefix.len()..]);
        }
    }
    if let Some((name, tail)) = path.strip_prefix('~').and_then(|tail| tail.split_once('/'))
        && !name.is_empty()
        && name != user.unwrap_or("unknown")
    {
        let candidate = format!("{home}/{tail}");
        if lexical_literal(&candidate, home).is_some() {
            return candidate;
        }
    }
    path.to_owned()
}
