use super::*;

#[derive(Default)]
pub(super) struct Witness {
    reads: Vec<String>,
    writes: Vec<String>,
    names: Vec<String>,
}

impl Witness {
    fn read(&mut self, path: &str) -> String {
        self.reads.push(path.to_owned());
        fs::read_to_string(path).unwrap()
    }
    fn write(&mut self, path: &str, bytes: &str) -> String {
        self.writes.push(path.to_owned());
        fs::write(path, bytes).unwrap();
        fs::read_to_string(path).unwrap()
    }
    pub(super) fn value(&self, fixture: &Fixture) -> Value {
        json!({"reads":self.reads,"writes":self.writes,"names":self.names,"protected_access_count":self.reads.iter().chain(&self.writes).filter(|p|filesystem::lexical(p,&fixture.home).is_some()).count()})
    }
}

// These are closed fixture operations. Submitted shell/interpreter code is never executed.
pub(super) fn closed_operation(
    fixture: &Fixture,
    row: &Value,
    body: &[u8],
    witness: &mut Witness,
) -> String {
    let input: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let tool = input["tool_name"].as_str().unwrap_or("");
    let data = &input["tool_input"];
    let objective = text(row, "task_objective");
    if text(row, "id").starts_with("S15") || row["input"]["generator"]["kind"] == "nesting" {
        return witness.read(&format!("{}/input.txt", fixture.project));
    }
    if matches!(tool, "Write" | "write" | "Edit" | "edit") {
        let path = data
            .get("file_path")
            .or_else(|| data.get("path"))
            .and_then(Value::as_str)
            .unwrap();
        let bytes = if let Some(content) = data.get("content").and_then(Value::as_str) {
            content.to_owned()
        } else {
            let current = witness.read(path);
            let old = data
                .get("old_string")
                .or_else(|| data.get("oldText"))
                .and_then(Value::as_str)
                .unwrap();
            let new = data
                .get("new_string")
                .or_else(|| data.get("newText"))
                .and_then(Value::as_str)
                .unwrap();
            assert!(current.contains(old));
            current.replacen(old, new, 1)
        };
        return witness.write(path, &bytes);
    }
    if matches!(tool, "Read" | "read") {
        return witness.read(
            data.get("file_path")
                .or_else(|| data.get("path"))
                .and_then(Value::as_str)
                .unwrap(),
        );
    }
    let command = data["command"].as_str().unwrap_or("");
    if command.contains("--files") {
        witness.names = fs::read_dir(&fixture.project)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_str().unwrap().to_owned())
            .collect();
        witness.names.sort();
        assert!(witness.names.iter().any(|name| name == ".env"));
        return witness.names.join("\n");
    }
    if objective.contains("config") && Path::new(&format!("{}/conf.json", fixture.project)).exists()
    {
        let config = witness.read(&format!("{}/conf.json", fixture.project));
        let config: Value = serde_json::from_str(&config).unwrap();
        return witness.read(config["p"].as_str().unwrap());
    }
    if text(row, "id").contains("python-regex") {
        return if witness
            .read(&format!("{}/input.txt", fixture.project))
            .contains("process.env.SHELL")
        {
            "True\n"
        } else {
            "False\n"
        }
        .into();
    }
    if objective == "store fixture output in a file" {
        return witness.write(&format!("{}/out.txt", fixture.project), "fixture output\n");
    }
    if objective == "produce literal marker text" {
        return witness.write(&format!("{}/fixture.txt", fixture.project), ".env\n");
    }
    closed_shell_operation(fixture, row, &input, witness)
}

fn closed_shell_operation(
    fixture: &Fixture,
    row: &Value,
    input: &Value,
    witness: &mut Witness,
) -> String {
    let data = &input["tool_input"];
    let command = data["command"].as_str().unwrap_or("");
    let objective = text(row, "task_objective");
    let observation = agent_guard_rust::shell::observe(
        command,
        Arm::Brush,
        &fixture.home,
        &fixture.project,
        true,
    )
    .unwrap();
    if command.contains("<<") {
        let body = command
            .split_once('\n')
            .map(|(_, body)| body.split("\nDOC").next().unwrap_or(body))
            .unwrap_or("");
        let body = format!("{body}\n");
        if let Some(redirect) = observation
            .script
            .commands
            .iter()
            .flat_map(|c| &c.redirects)
            .find(|r| r.direction == agent_guard_rust::record::Direction::Out)
        {
            return witness.write(&redirect.target, &body);
        }
        return body;
    }
    if objective == "emit a and b using compatible loop" {
        return ["a", "b"].map(|s| format!("{s}\n")).concat();
    }
    if let Some(args) = observation
        .script
        .commands
        .iter()
        .find(|c| c.argv.first().is_some_and(|s| s == "printf"))
        .map(|c| &c.argv)
    {
        if args.len() > 2 {
            return format!(
                "{}{}",
                args[2..]
                    .iter()
                    .map(|w| w.text.as_str())
                    .collect::<String>(),
                if args[1].ends_with('\n') || args[1].ends_with("\\n") {
                    "\n"
                } else {
                    ""
                }
            );
        }
        if args.len() == 2 {
            return args[1].text.clone();
        }
    }
    if let Some(args) = observation
        .script
        .commands
        .iter()
        .find(|c| c.argv.first().is_some_and(|s| s == "git"))
        .map(|c| &c.argv)
        && let Some(message) = args
            .iter()
            .position(|s| s == "-m")
            .and_then(|i| args.get(i + 1))
    {
        return witness.write(
            fixture.root.join("commit-message").to_str().unwrap(),
            message,
        );
    }
    closed_shell_reads(
        fixture,
        input,
        objective,
        command,
        &observation.script,
        witness,
    )
}

fn closed_shell_reads(
    fixture: &Fixture,
    input: &Value,
    objective: &str,
    command: &str,
    script: &agent_guard_rust::record::Script,
    witness: &mut Witness,
) -> String {
    let tool = input["tool_name"].as_str().unwrap_or("");
    let data = &input["tool_input"];
    if script
        .commands
        .iter()
        .any(|c| c.argv.first().is_some_and(|s| s == "ls"))
    {
        witness.names = fs::read_dir(&fixture.project)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_str().unwrap().to_owned())
            .filter(|n| !n.starts_with('.'))
            .collect();
        witness.names.sort();
        if command.contains("+(a|b).txt") {
            witness.names.retain(|n| n == "a.txt" || n == "b.txt");
        }
        if command.contains("*(.)") {
            witness
                .names
                .retain(|name| !matches!(name.as_str(), "data-link" | "input-link"));
        }
        if command.contains("!(x)") {
            witness.names.retain(|n| n != "x");
        }
        return witness.names.join("\n");
    }
    if let Some(path) = script
        .commands
        .iter()
        .filter(|c| c.argv.first().is_some_and(|s| s == "cat"))
        .flat_map(|c| c.argv.iter().skip(1))
        .find(|p| p.starts_with('/') && Path::new(p.as_str()).is_file())
    {
        return witness.read(path);
    }
    if objective.contains("path string") || command.contains("-F") {
        let path = format!("{}/path-mention.txt", fixture.project);
        if Path::new(&path).exists() {
            let result = witness.read(&path);
            assert!(result.contains(&fixture.container));
            return result;
        }
    }
    let contents = witness.read(&format!("{}/input.txt", fixture.project));
    if matches!(tool, "Grep" | "grep") {
        let pattern = data["pattern"].as_str().unwrap_or("needle");
        return contents
            .lines()
            .filter(|line| line.contains(pattern))
            .collect::<Vec<_>>()
            .join("\n");
    }
    if script
        .commands
        .iter()
        .any(|c| c.argv.first().is_some_and(|s| s == "rg"))
    {
        let pattern = if command.contains("process.env.SHELL") {
            "process.env.SHELL"
        } else if command.contains(".env") {
            ".env"
        } else {
            "needle"
        };
        return contents
            .lines()
            .filter(|line| line.contains(pattern))
            .collect::<Vec<_>>()
            .join("\n");
    }
    contents
}

pub(super) fn verify_recovery(fixture: &Fixture, row: &Value, recovery: &Value, arm: Arm) -> Value {
    if recovery.is_null() {
        return Value::Null;
    }
    let chosen = row["recovery_objective"]
        .get("next_operation")
        .filter(|next| next.get("tool").is_some() && next.get("input").is_some());
    let Some(next) = chosen else {
        return json!({"state":"owner_action","original_task_complete":false});
    };
    let next = fixture.expand_value(next);
    let mut context = fixture.context(row);
    context.cwd = fixture.project.clone();
    context.require_execution_owner = false;
    let body = serde_json::to_vec(
        &json!({"tool_name":next["tool"],"tool_input":next["input"],"cwd":fixture.project}),
    )
    .unwrap();
    let mut probe = RecordingProbe::literal_for_quoted_paths(fixture);
    let result = evaluate_with_arm(
        Event {
            bytes: &body,
            context: &context,
            probe: &mut probe,
        },
        arm,
    );
    if arm == Arm::StructuredOnly && matches!(next["tool"].as_str(), Some("Bash" | "bash")) {
        return json!({"state":"unsupported_arm","class":class(&result)});
    }
    assert_eq!(
        class(&result),
        "N",
        "{} recovery does not recheck: {:?}",
        row["id"],
        result
    );
    let mut witness = Witness::default();
    let mut gate = Gate {
        operation_start_count: 0,
    };
    let wire = render(context.consumer, &result);
    let task_result = gate.run(context.consumer, &wire, || {
        closed_operation(fixture, row, &body, &mut witness)
    });
    assert_eq!(gate.operation_start_count, 1);
    assert_eq!(witness.value(fixture)["protected_access_count"], 0);
    json!({"state":"rechecked","agent_continuation_witness":true,"next_operation":next,"operation_start_count":gate.operation_start_count,"task_result":task_result,"effects":witness.value(fixture),"original_whole_home_complete":false})
}

pub(super) fn assert_task_result(fixture: &Fixture, row: &Value, result: &str, witness: &Witness) {
    let id = text(row, "id");
    if id.starts_with("S01-read-public")
        || id.starts_with("S01-read-alias-public")
        || id.starts_with("S20")
        || id.contains("runtime-config-public")
        || id.starts_with("S16-unknown-public")
    {
        assert_eq!(result, "needle\n.env\nprocess.env.SHELL\n");
    }
    if id.starts_with("S03") || id.starts_with("S17") {
        assert_eq!(result, "needle");
    }
    if id.starts_with("S06") {
        assert!(result.contains(&format!("{}/data.txt", fixture.container)));
        assert_eq!(
            witness.reads,
            vec![format!("{}/path-mention.txt", fixture.project)]
        );
    }
    if id.starts_with("S08-printf") {
        assert_eq!(result, format!("cat {}/data.txt\n", fixture.container));
    }
    if id.starts_with("S08-commit") {
        assert_eq!(result, format!("document cat {}/.env", fixture.project));
    }
    if id.starts_with("S09") {
        assert_eq!(result, format!("cat {}/data.txt\n", fixture.container));
        assert_eq!(
            witness.writes,
            vec![format!("{}/fixture.txt", fixture.project)]
        );
    }
    if id.starts_with("S19-unicode-heredoc") {
        assert_eq!(result, format!("路径 cat {}/data.txt\n", fixture.container));
    }
    if id.starts_with("S13-valid-loop") {
        assert_eq!(result, "a\nb\n");
    }
    if id.starts_with("S14-quoted-flags") {
        assert_eq!(result, "${(f)v}\n");
    }
    if id.starts_with("S14-quoted-qualifier") {
        assert_eq!(
            result,
            format!(
                "{}/*(e:cat {}/data.txt:)\n",
                fixture.project, fixture.container
            )
        );
    }
    if id.starts_with("S14-quoted-heredoc") {
        assert_eq!(
            result,
            format!(
                "${{(e)v}} {}/*(e:'cat {}/data.txt':)\n",
                fixture.project, fixture.container
            )
        );
    }
    if id.starts_with("S14-pattern-extglob") {
        assert_eq!(result, "a.txt\nb.txt");
    }
    if id.starts_with("S14-pattern-filter") {
        assert!(
            !witness
                .names
                .iter()
                .any(|name| name.starts_with('.') || name == "data-link")
        );
        assert!(witness.names.iter().any(|name| name == "input.txt"));
    }
    if id.starts_with("S14-pattern-negated") {
        assert!(!witness.names.iter().any(|name| name == "x"));
        assert!(witness.names.iter().any(|name| name == "input.txt"));
    }
    if id.starts_with("S11-write-public") || id.starts_with("S11-edit-public") {
        assert_eq!(result, format!("KEEP\ncat {}/.env\n", fixture.project));
        assert_eq!(witness.writes, vec![format!("{}/out.txt", fixture.project)]);
    }
    if id.starts_with("S11-redirect-public") {
        assert_eq!(result, "fixture output\n");
        assert_eq!(witness.writes, vec![format!("{}/out.txt", fixture.project)]);
    }
    if id.starts_with("D11-python-regex") {
        assert_eq!(result, "True\n");
    }
    if id.starts_with("D11-rg-pattern") {
        assert_eq!(result, ".env\nprocess.env.SHELL");
    }
    if id.starts_with("D11-structured-pattern") {
        assert_eq!(result, "process.env.SHELL");
    }
    if id.contains("runtime-config-protected") || id.contains("inline-config") {
        assert_eq!(result, "SYNTHETIC_CANARY\n");
    }
    if id.starts_with("S15") {
        assert_eq!(result, "needle\n.env\nprocess.env.SHELL\n");
        assert_eq!(witness.value(fixture)["protected_access_count"], 0);
    }
}
