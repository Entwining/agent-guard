use super::*;

pub(super) struct AcceptanceCase {
    pub(super) name: &'static str,
    pub(super) tool: &'static str,
    pub(super) input: serde_json::Value,
    pub(super) expected: i32,
    pub(super) reason: &'static str,
    pub(super) advice: &'static str,
}

#[expect(
    clippy::too_many_lines,
    reason = "This flat acceptance table keeps each input beside its expected exit, reason and advice."
)]
pub(super) fn cases(home: &Path, cwd: &Path) -> Vec<AcceptanceCase> {
    use serde_json::json;
    let mut rows = Vec::new();
    let mut add = |name, tool, input, expected, reason, advice| {
        rows.push(AcceptanceCase {
            name,
            tool,
            input,
            expected,
            reason,
            advice,
        })
    };
    add(
        "project-shell",
        "Bash",
        json!({"command":"cat file.txt"}),
        0,
        "",
        "",
    );
    add(
        "project-read",
        "Read",
        json!({"file_path":cwd.join("file.txt")}),
        0,
        "",
        "",
    );
    add(
        "environment-write",
        "Write",
        json!({"file_path":cwd.join(".env"),"content":""}),
        0,
        "",
        "",
    );
    add(
        "client-key-use",
        "Bash",
        json!({"command":"ssh -i ~/.ssh/id_rsa example.invalid"}),
        0,
        "",
        "",
    );
    add(
        "public-key-read",
        "Read",
        json!({"file_path":home.join(".ssh/id.pub")}),
        0,
        "",
        "",
    );
    add(
        "project-search",
        "Grep",
        json!({"path":cwd.join("src"),"pattern":"canary"}),
        0,
        "",
        "",
    );
    add(
        "claude-advice",
        "Bash",
        json!({"command":"rg -rn canary src"}),
        0,
        "",
        ADVICE,
    );
    add(
        "appdata-shell",
        "Bash",
        json!({"command":"cat ~/Library/Containers/com.example.canary/file.txt"}),
        2,
        "This reads a protected macOS app-data directory.",
        "",
    );
    add(
        "appdata-read",
        "Read",
        json!({"file_path":home.join("Library/Containers/com.example.canary/file.txt")}),
        2,
        "This reads a protected macOS app-data directory.",
        "",
    );
    add(
        "appdata-link",
        "Bash",
        json!({"command":"cat data-link/com.example.canary/file.txt"}),
        2,
        "This reads a protected macOS app-data directory.",
        "",
    );
    add(
        "environment-read",
        "Bash",
        json!({"command":"cat .env"}),
        2,
        "This reads a credential or environment file.",
        "",
    );
    add(
        "private-key-read",
        "Read",
        json!({"file_path":home.join(".ssh/id_rsa")}),
        2,
        "This reads a credential or environment file.",
        "",
    );
    add(
        "broad-scan-advice",
        "Bash",
        json!({"command":"rg -rn canary ~/Library"}),
        2,
        "A scan rooted at the home directory or ~/Library",
        "",
    );
    rows
}

pub(super) fn mismatches(
    result: &ResultOutput,
    row: &AcceptanceCase,
    runtime: &str,
) -> Vec<String> {
    let mut problems = Vec::new();
    if result.timed_out {
        problems.push("deadline exceeded".into());
    }
    if let Some(signal) = result.signal {
        let names = [
            "",
            "hangup",
            "interrupt",
            "quit",
            "illegal instruction",
            "trace/BPT trap",
            "abort trap",
            "EMT trap",
            "floating point exception",
            "killed",
            "bus error",
            "segmentation fault",
            "bad system call",
            "broken pipe",
            "alarm clock",
            "terminated",
            "urgent I/O condition",
            "suspended (signal)",
            "suspended",
            "continued",
            "child exited",
            "stopped (tty input)",
            "stopped (tty output)",
            "I/O possible",
            "cputime limit exceeded",
            "filesize limit exceeded",
            "virtual timer expired",
            "profiling timer expired",
            "window size changes",
            "information request",
            "user defined signal 1",
            "user defined signal 2",
        ];
        let name = names
            .get(signal as usize)
            .map(|name| (*name).to_owned())
            .unwrap_or_else(|| format!("signal {signal}"));
        problems.push(format!("signal {name}"));
    }
    if result.exit != row.expected {
        problems.push(format!("expected exit {}", row.expected));
    }
    if row.expected == 2 {
        if result.stderr.trim().is_empty() {
            problems.push("empty denial reason".into());
        }
        if !result.stderr.contains(row.reason) {
            problems.push("wrong denial reason".into());
        }
        if runtime == "claude" && !result.stderr.starts_with("DENIED: ") {
            problems.push("missing Claude denial prefix".into());
        }
        if !result.stdout.trim().is_empty() {
            problems.push("denial emitted advice or unexpected stdout".into());
        }
    } else {
        if !result.stderr.trim().is_empty() {
            problems.push("unexpected stderr on allow".into());
        }
        if runtime == "claude" && !row.advice.is_empty() {
            match claude_advice(&result.stdout, row.advice) {
                Err(()) => problems.push("invalid Claude advice JSON".into()),
                Ok(false) => problems.push("missing Claude advice".into()),
                Ok(true) => {}
            }
        } else if !result.stdout.trim().is_empty() {
            problems.push("unexpected advice on allow".into());
        }
    }
    problems
}

fn claude_advice(output: &str, advice: &str) -> Result<bool, ()> {
    let output = serde_json::from_str::<AdviceOutput>(output).map_err(|_| ())?;
    Ok(output.0.event == "PreToolUse" && output.0.context == advice)
}

#[derive(Default)]
struct HookAdvice {
    event: String,
    context: String,
}

struct AdviceOutput(HookAdvice);

fn field_matches(key: &str, name: &str) -> bool {
    key.chars()
        .map(|character| match character {
            '\u{017f}' => 's',
            '\u{212a}' => 'k',
            _ => character.to_ascii_lowercase(),
        })
        .eq(name.chars().map(|character| character.to_ascii_lowercase()))
}

impl<'de> serde::Deserialize<'de> for AdviceOutput {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct OutputVisitor;
        impl<'de> serde::de::Visitor<'de> for OutputVisitor {
            type Value = AdviceOutput;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a hook output object or null")
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(AdviceOutput(HookAdvice::default()))
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<Self::Value, M::Error> {
                let mut hook = HookAdvice::default();
                while let Some(key) = map.next_key::<String>()? {
                    if field_matches(&key, "hookSpecificOutput") {
                        map.next_value_seed(HookSeed(&mut hook))?;
                    } else {
                        map.next_value::<serde::de::IgnoredAny>()?;
                    }
                }
                Ok(AdviceOutput(hook))
            }
        }
        deserializer.deserialize_any(OutputVisitor)
    }
}

struct HookSeed<'a>(&'a mut HookAdvice);

impl<'de> serde::de::DeserializeSeed<'de> for HookSeed<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> serde::de::Visitor<'de> for HookSeed<'_> {
    type Value = ();
    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a hook-specific output object or null")
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<(), E> {
        Ok(())
    }
    fn visit_map<M: serde::de::MapAccess<'de>>(self, mut map: M) -> Result<(), M::Error> {
        // Merge repeated fields in input order; null preserves the prior string.
        while let Some(key) = map.next_key::<String>()? {
            let target = if field_matches(&key, "hookEventName") {
                Some(&mut self.0.event)
            } else if field_matches(&key, "additionalContext") {
                Some(&mut self.0.context)
            } else {
                None
            };
            if let Some(target) = target {
                if let Some(value) = map.next_value::<Option<String>>()? {
                    *target = value;
                }
            } else {
                map.next_value::<serde::de::IgnoredAny>()?;
            }
        }
        Ok(())
    }
}
