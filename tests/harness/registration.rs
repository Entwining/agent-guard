use super::{Result, process::*};
use serde_json::json;
use std::path::Path;

/// The executable the setup guide registers.
const GUARD: &str = "/opt/homebrew/bin/agent-guard";

/// The first `fence` code block under the setup guide's `heading`, with the
/// guard path inside its string literal replaced by `guard`.
pub fn registration(guide: &str, heading: &str, fence: &str, guard: &str) -> Result<String> {
    let section = guide
        .split_once(&format!("\n## {heading}\n"))
        .and_then(|(_, rest)| rest.split("\n## ").next())
        .ok_or_else(|| format!("the setup guide has no {heading:?} section"))?;
    let block = section
        .split_once(&format!("\n```{fence}\n"))
        .and_then(|(_, rest)| rest.split_once("\n```"))
        .ok_or_else(|| format!("{heading:?} has no {fence} block"))?
        .0;
    if !block.contains(GUARD) {
        return Err(format!("{heading:?} does not register {GUARD}").into());
    }
    let literal = serde_json::to_string(guard)?;
    Ok(block.replace(GUARD, &literal[1..literal.len() - 1]))
}
pub fn runtime_args(runtime: &str, client: &Path, home: &Path) -> Vec<String> {
    let args: Vec<String> = match runtime {
        "pi" => vec![
            "--no-session".into(),
            "-e".into(),
            home.join("pi-provider.mjs").to_string_lossy().into(),
            // Pi leaves grep out of its default tools.
            "--tools".into(),
            "+grep".into(),
            "--provider".into(),
            "harness".into(),
            "--model".into(),
            "synthetic".into(),
            "--mode".into(),
            "json".into(),
            "-p".into(),
            "Run the requested tool.".into(),
        ],
        "claude" => [
            "-p",
            "Run the requested tool.",
            // Claude Code makes older models, such as claude-sonnet-4-5, read a
            // file before editing it, and the Edit case edits an unread file.
            "--model",
            "claude-sonnet-5-5",
            "--output-format",
            "stream-json",
            "--verbose",
            "--dangerously-skip-permissions",
            // Claude Code's native build offers Grep only when a launch option names it.
            "--allowedTools",
            "Grep",
        ]
        .map(String::from)
        .to_vec(),
        _ => [
            "--dangerously-bypass-hook-trust",
            "exec",
            "--skip-git-repo-check",
            "--json",
            "Run the requested tool.",
        ]
        .map(String::from)
        .to_vec(),
    };
    std::iter::once(client.to_string_lossy().into_owned())
        .chain(args)
        .collect()
}
/// Registers `helper` as the guard: Claude Code and Pi through the setup
/// guide's registration, Codex through the Bash hook its managed requirements
/// declare.
pub fn runtime_config(
    home: &Path,
    runtime: &str,
    client: &Path,
    helper: &Path,
    url: &str,
    guide: &str,
) -> Result<()> {
    let helper_path = helper.to_string_lossy();
    match runtime {
        "claude" => write_file(
            &home.join(".claude/settings.json"),
            registration(guide, "Register Claude Code", "json", &quote(&helper_path))?.as_bytes(),
            0o600,
        ),
        "pi" => {
            write_file(
                &home.join(".pi/agent/extensions/agent-guard.ts"),
                registration(guide, "Register Pi", "ts", &helper_path)?.as_bytes(),
                0o600,
            )?;
            write_file(
                &home.join("pi-provider.mjs"),
                include_bytes!("testdata/pi-provider.mjs"),
                0o600,
            )
        }
        _ => {
            let config = json!({"hooks":{"PreToolUse":[{"matcher":"^Bash$","hooks":[{"type":"command","command":format!("{} --runtime codex",quote(&helper_path)),"timeout":5}]}]}});
            write_json(&home.join(".codex/hooks.json"), &config)?;
            let mut reads = format!("{} = \"read\"\n", serde_json::to_string(&helper_path)?);
            if client.is_absolute() {
                reads += &format!(
                    "{} = \"read\"\n",
                    serde_json::to_string(&client.to_string_lossy())?
                );
            }
            let config = format!(
                "model = \"synthetic\"\nmodel_provider = \"harness\"\napproval_policy = \"on-request\"\napprovals_reviewer = \"auto_review\"\ndefault_permissions = \"development\"\n[features]\nhooks = true\n[permissions.development.filesystem]\n\":minimal\" = \"read\"\n{} = \"write\"\n{reads}[permissions.development.network]\nenabled = true\n[model_providers.harness]\nname = \"Synthetic local harness\"\nbase_url = {}\nwire_api = \"responses\"\nrequires_openai_auth = false\n",
                serde_json::to_string(&home.to_string_lossy())?,
                serde_json::to_string(&format!("{url}/v1"))?
            );
            write_file(&home.join(".codex/config.toml"), config.as_bytes(), 0o600)
        }
    }
}
