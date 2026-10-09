#![forbid(unsafe_code)]

use std::{
    fs, io,
    os::unix::process::CommandExt,
    path::Path,
    process::{Command, ExitCode},
};

const ADVICE: &str = "rg -r means --replace. Drop -r; use -n for line numbers, or spell --replace VALUE for an intentional replacement.";

fn fixture(args: &[String]) -> Result<u8, Box<dyn std::error::Error>> {
    if args.get(1).is_some_and(|value| value == "--version") {
        println!("agent-guard 0.0.0");
        return Ok(0);
    }
    let fault = args.first().ok_or("missing fault")?;
    if matches!(fault.as_str(), "hang" | "orphan-pipe" | "signal-burst") {
        let mut child = Command::new("/bin/sleep").arg("20").spawn()?;
        let delayed_pipe = if fault == "signal-burst" {
            Some(
                Command::new("/bin/sleep")
                    .arg("0.2")
                    .process_group(0)
                    .spawn()?,
            )
        } else {
            None
        };
        let temporary = std::env::var("TMPDIR")?;
        let root = Path::new(&temporary)
            .parent()
            .ok_or("missing fixture root")?;
        let mut pids = format!("{} {}", std::process::id(), child.id());
        if let Some(pipe) = &delayed_pipe {
            pids.push_str(&format!(" {}", pipe.id()));
        }
        fs::write(root.join("fixture-pids"), pids)?;
        fs::write(
            root.join("fixture-started"),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
                .to_string(),
        )?;
        if fault != "orphan-pipe" {
            child.wait()?;
        }
        return Ok(0);
    }
    let runtime = args.get(2).ok_or("missing runtime")?;
    let event: serde_json::Value = serde_json::from_reader(io::stdin())?;
    let tool = event["tool_name"].as_str().ok_or("missing tool")?;
    let input = &event["tool_input"];
    if fault == "event-shape"
        && (event["cwd"].as_str().is_none_or(str::is_empty)
            || input.get("cwd").is_some()
            || runtime == "codex" && tool != "Bash"
            || runtime == "pi" && tool != tool.to_lowercase())
    {
        return Ok(7);
    }
    let text = ["command", "file_path", "path"]
        .iter()
        .map(|key| input[key].as_str().unwrap_or(""))
        .collect::<String>();
    let mut reason = if text.starts_with("rg") && text.contains("~/Library") {
        "A scan rooted at the home directory or ~/Library"
    } else if text.contains("Library/Containers") || text.contains("data-link") {
        "This reads a protected macOS app-data directory."
    } else if !tool.eq_ignore_ascii_case("write")
        && (text.ends_with(".env") || text.ends_with("id_rsa") && !text.starts_with("ssh"))
    {
        "This reads a credential or environment file."
    } else {
        ""
    };
    if fault == "all-allow" {
        reason = "";
    }
    if fault == "all-deny" {
        reason = "Synthetic unconditional denial";
    }
    if !reason.is_empty() {
        if fault == "wrong-reason" {
            reason = "A different denial";
        }
        if fault != "empty-reason" {
            eprintln!(
                "{}{reason}",
                if runtime == "claude" && fault != "missing-prefix" {
                    "DENIED: "
                } else {
                    ""
                }
            );
        }
        if fault == "denial-stdout" {
            println!("Unexpected denial advice");
        }
        return Ok(if fault == "status" { 7 } else { 2 });
    }
    if fault == "allow-stderr" {
        eprintln!("Unexpected allow stderr");
    }
    if text.starts_with("rg -rn")
        && fault != "missing-advice"
        && (runtime == "claude" || fault == "non-claude-advice")
    {
        let advice = if fault == "changed-advice" {
            format!("{ADVICE} Unexpected extra advice.")
        } else {
            ADVICE.to_owned()
        };
        match fault.as_str() {
            "advice-scalar" => println!("42"),
            "advice-field-type" => println!("{{\"hookSpecificOutput\":{{\"hookEventName\":42}}}}"),
            "advice-duplicate" => println!(
                "{{\"hookSpecificOutput\":{{\"additionalContext\":{}}},\"HOOKSPECIFICOUTPUT\":{{\"hookEventName\":\"PreToolUse\",\"additionalContext\":null}}}}",
                serde_json::to_string(&advice)?
            ),
            "advice-case-fold" => println!(
                "{{\"HOOKſPECIFICOUTPUT\":{{\"HOOKEVENTNAME\":\"PreToolUse\",\"ADDITIONALCONTEXT\":{}}}}}",
                serde_json::to_string(&advice)?
            ),
            _ => println!(
                "{}",
                serde_json::json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":advice}})
            ),
        }
    }
    Ok(0)
}

fn main() -> ExitCode {
    match fixture(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(7)
        }
    }
}
