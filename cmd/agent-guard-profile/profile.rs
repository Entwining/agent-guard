use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
    process::Command,
    sync::atomic::AtomicUsize,
    time::Duration,
};

use crate::public_tools::{execute, path_error};

const INSTRUCTIONS: &str = include_str!("instructions.txt");

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn escape_xml(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '&' => "&amp;".into(),
            '<' => "&lt;".into(),
            '>' => "&gt;".into(),
            '\'' => "&#39;".into(),
            '"' => "&#34;".into(),
            '\t' => "&#x9;".into(),
            '\n' => "&#xA;".into(),
            '\r' => "&#xD;".into(),
            _ => character.to_string(),
        })
        .collect()
}

fn random_uuid() -> Result<String, String> {
    let mut bytes = [0; 16];
    fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|error| error.to_string())?;
    bytes[6] = bytes[6] & 0x0f | 0x40;
    bytes[8] = bytes[8] & 0x3f | 0x80;
    let hex = bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<String>();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
}

fn validate(path: &str, args: &[&str]) -> Result<(), String> {
    let result = execute(
        Command::new(path).args(args),
        &[],
        Duration::from_secs(2),
        Duration::from_millis(250),
        &AtomicUsize::new(0),
        true,
    )?;
    if result.exit != 0 || result.signal.is_some() || result.timed_out {
        let status = if result.signal.is_some() {
            "signal: killed".into()
        } else {
            format!("exit status {}", result.exit)
        };
        return Err(format!(
            "{status}: {}",
            format!("{}{}", result.stdout, result.stderr).trim()
        ));
    }
    Ok(())
}

pub fn write_profile(output: &Path, profile: &str) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(output)
        .map_err(|error| path_error("open", output, &error))?;
    file.write_all(profile.as_bytes())
        .map_err(|error| path_error("write", output, &error))?;
    nix::unistd::close(file).map_err(|error| path_error("close", output, &error.into()))?;
    let output = output.to_str().ok_or("output path is not UTF-8")?;
    validate("/usr/bin/plutil", &["-lint", output])
        .map_err(|error| format!("profile written but validation failed: {error}"))
}

pub fn generate(args: &[String], stdout: &mut dyn Write) -> Result<(), String> {
    if args == ["--instructions"] {
        return stdout
            .write_all(INSTRUCTIONS.as_bytes())
            .map_err(|error| error.to_string());
    }
    let [type_name, identifier, requirement, output] = args else {
        return Err("expected identifier type, identifier, designated requirement, and absolute output path; use --instructions for exact steps".into());
    };
    if type_name != "bundleID" && type_name != "path" {
        return Err("IdentifierType must be bundleID or path".into());
    }
    for text in [identifier, requirement] {
        if text.trim().is_empty() || text.chars().any(|character| character < ' ') {
            return Err(
                "identity and requirement must be nonempty text without control characters".into(),
            );
        }
    }
    if type_name == "path" && !Path::new(identifier).is_absolute() {
        return Err("a binary identifier must be an absolute installation path".into());
    }
    if type_name == "bundleID"
        && !identifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    {
        return Err("invalid bundle ID".into());
    }
    if requirement.contains("designated =>") {
        return Err("supply only the requirement after \"designated =>\"".into());
    }
    let output = Path::new(output);
    if !output.is_absolute() {
        return Err("output must be an absolute path outside checkouts".into());
    }
    let parent_path = output.parent().ok_or("missing output parent")?;
    #[expect(
        clippy::disallowed_methods,
        reason = "The profile output owner resolves its parent before excluding checkout aliases."
    )]
    let parent =
        fs::canonicalize(parent_path).map_err(|error| path_error("lstat", parent_path, &error))?;
    for directory in parent.ancestors() {
        let marker = directory.join(".git");
        #[expect(
            clippy::disallowed_methods,
            reason = "The profile output owner checks Git markers without following a marker symlink."
        )]
        match fs::symlink_metadata(&marker) {
            Ok(_) => return Err("output must be outside every checkout".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(path_error("lstat", &marker, &error)),
        }
    }
    validate("/usr/bin/csreq", &["-r", &format!("={requirement}"), "-t"])
        .map_err(|error| format!("invalid code requirement: {error}"))?;
    let uuid = random_uuid()?;
    let payload_uuid = random_uuid()?;
    let payload_identifier = format!("com.loophubs.agent-guard.appdata-deny.{uuid}");
    let profile = format!(
        include_str!("profile.xml"),
        uuid,
        payload_identifier,
        payload_uuid,
        payload_identifier,
        escape_xml(identifier),
        type_name,
        escape_xml(requirement)
    );
    write_profile(output, &profile)?;
    writeln!(stdout, "Unsigned profile: {}\nPayloadIdentifier: {payload_identifier}\nSelected client: {type_name} {identifier}\nPlist validation: PASS\nRuntime enforcement: unverified", output.display()).map_err(|error| error.to_string())?;
    let quoted = quote(&output.to_string_lossy());
    writeln!(stdout, "Inspect this artifact: /usr/bin/plutil -p {quoted}\nInspect its exact denial: /usr/bin/plutil -extract PayloadContent.0.Services.SystemPolicyAppData.0.Allowed raw -o - {quoted}").map_err(|error| error.to_string())?;
    stdout
        .write_all(INSTRUCTIONS.as_bytes())
        .map_err(|error| error.to_string())
}
