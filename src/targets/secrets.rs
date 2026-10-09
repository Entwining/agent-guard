use super::{Effects, clients::CURL_VALUE_LETTERS, secret_name};
use crate::record::Word;

pub(super) fn infer(program: &str, args: &[Word], effects: &mut Effects) {
    let arg = |index| args.get(index).map_or("", Word::as_str);
    match program {
        "gh" => {
            effects.token = arg(0) == "auth"
                && (arg(1) == "token" || arg(1) == "status" && gh_shows_token(&args[2..]));
        }
        "glab" => {
            effects.token = arg(0) == "auth"
                && arg(1) == "status"
                && args.iter().any(|arg| {
                    arg.starts_with("--show-token")
                        || arg.strip_prefix('-').is_some_and(|flags| {
                            flags.chars().take_while(|c| *c != '-').any(|c| c == 't')
                        })
                });
        }
        "security" => {
            effects.keychain = ["dump-keychain", "export"].contains(&arg(0))
                || args.iter().any(|arg| {
                    arg.strip_prefix('-').is_some_and(|flags| {
                        flags
                            .chars()
                            .take_while(char::is_ascii_alphabetic)
                            .any(|c| matches!(c, 'w' | 'g'))
                    })
                });
        }
        "curl" => effects.trace = curl_traces(args),
        _ => {}
    }
    effects.stored_secret = match program {
        "gcloud" => args
            .iter()
            .any(|arg| ["print-access-token", "print-identity-token"].contains(&arg.as_str())),
        "az" => arg(0) == "account" && arg(1) == "get-access-token",
        "aws" => arg(0) == "configure" && arg(1) == "get" && secret_name(arg(2)),
        "npm" => {
            arg(0) == "config"
                && arg(1) == "get"
                && ["auth", "token", "password"]
                    .iter()
                    .any(|name| arg(2).to_ascii_lowercase().contains(name))
        }
        "kubectl" => {
            arg(0) == "config" && arg(1) == "view" && args.iter().any(|arg| arg == "--raw")
        }
        "gpg" => args
            .iter()
            .any(|arg| ["--export-secret-keys", "--export-secret-subkeys"].contains(&arg.as_str())),
        _ => false,
    };
}

fn gh_shows_token(args: &[Word]) -> bool {
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        if arg == "--" {
            return false;
        }
        if ["--hostname", "--jq", "--json", "--template", "-h"].contains(&arg.as_str()) {
            index += 1;
        } else if arg == "--show-token"
            || arg.starts_with("--show-token=")
            || arg.strip_prefix('-').is_some_and(|flags| {
                flags
                    .chars()
                    .take_while(|c| matches!(c, 'a' | 't'))
                    .any(|c| c == 't')
            })
        {
            return true;
        }
        index += 1;
    }
    false
}

fn curl_traces(args: &[Word]) -> bool {
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        if arg == "--" {
            break;
        }
        let key = arg.split('=').next().unwrap_or("");
        if ["--verbose", "--trace", "--trace-ascii"].contains(&key) {
            return true;
        }
        if [
            "--data",
            "--data-ascii",
            "--data-binary",
            "--data-urlencode",
            "--json",
            "--form",
            "--header",
            "--upload-file",
            "--config",
        ]
        .contains(&arg.as_str())
        {
            index += 1;
        }
        if let Some(flags) = arg
            .strip_prefix('-')
            .filter(|flags| !flags.starts_with('-'))
        {
            for (at, letter) in flags.char_indices() {
                if letter == 'v' {
                    return true;
                }
                if CURL_VALUE_LETTERS.contains(letter) {
                    if at + letter.len_utf8() == flags.len() {
                        index += 1;
                    }
                    break;
                }
            }
        }
        index += 1;
    }
    false
}
