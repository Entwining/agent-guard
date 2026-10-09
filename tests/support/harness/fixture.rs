use super::*;

pub fn rows() -> Vec<Value> {
    include_str!("../../fixtures/rust-slice-dev.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

pub fn text<'a>(value: &'a Value, field: &str) -> &'a str {
    value[field].as_str().unwrap()
}

pub struct Fixture {
    pub root: PathBuf,
    pub home: String,
    pub project: String,
    pub container: String,
}

impl Fixture {
    pub fn new_at(scratch: &Path) -> Self {
        let root = scratch.join("fixtures").join(format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let home = root.join("home").to_str().unwrap().to_owned();
        let project = format!("{home}/project");
        let container = format!("{home}/Library/Containers/com.example.fixture");
        for dir in [&project, &container, &format!("{home}/.ssh")] {
            fs::create_dir_all(dir).unwrap();
        }
        for (path, bytes) in [
            (
                format!("{project}/input.txt"),
                "needle\n.env\nprocess.env.SHELL\n",
            ),
            (format!("{project}/notes.txt"), "ORIGINAL_NOTES\n"),
            (format!("{project}/untracked-work.txt"), "UNTRACKED_WORK\n"),
            (format!("{project}/out.txt"), "KEEP\n"),
            (format!("{project}/.env"), "SYNTHETIC_CANARY\n"),
            (format!("{home}/.ssh/id_rsa"), "SYNTHETIC_CANARY\n"),
            (format!("{container}/data.txt"), "SYNTHETIC_CANARY\n"),
        ] {
            fs::write(path, bytes).unwrap();
        }
        std::os::unix::fs::symlink(
            format!("{home}/Library/Containers"),
            format!("{project}/data-link"),
        )
        .unwrap();
        Self {
            root,
            home,
            project,
            container,
        }
    }
    pub fn expand(&self, value: &str) -> String {
        let mut result = value.to_owned();
        for (marker, replacement) in [
            ("$C", self.container.as_str()),
            ("$P", self.project.as_str()),
            ("$H", self.home.as_str()),
            ("$E", self.root.to_str().unwrap()),
            ("$R", self.root.to_str().unwrap()),
            ("$U", "fixture-user"),
        ] {
            let mut cursor = 0;
            while let Some(relative) = result[cursor..].find(marker) {
                let start = cursor + relative;
                let end = start + marker.len();
                if result
                    .as_bytes()
                    .get(end)
                    .is_none_or(|c| !c.is_ascii_alphanumeric() && *c != b'_')
                {
                    result.replace_range(start..end, replacement);
                    cursor = start + replacement.len();
                } else {
                    cursor = end;
                }
            }
        }
        result
    }
    pub fn expand_value(&self, value: &Value) -> Value {
        match value {
            Value::String(s) => Value::String(self.expand(s)),
            Value::Array(a) => Value::Array(a.iter().map(|v| self.expand_value(v)).collect()),
            Value::Object(o) => Value::Object(
                o.iter()
                    .map(|(k, v)| (k.clone(), self.expand_value(v)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
    pub fn setup(&self, row: &Value) {
        if let Some(files) = row["synthetic_setup"]["files"].as_object() {
            for (path, bytes) in files {
                let path = self.expand(path);
                fs::create_dir_all(Path::new(&path).parent().unwrap()).unwrap();
                fs::write(path, self.expand(bytes.as_str().unwrap())).unwrap();
            }
        }
        if let Some(links) = row["synthetic_setup"]["links"].as_array() {
            for pair in links {
                std::os::unix::fs::symlink(
                    self.expand(pair[1].as_str().unwrap()),
                    self.expand(pair[0].as_str().unwrap()),
                )
                .unwrap();
            }
        }
    }
    pub fn context(&self, row: &Value) -> Context {
        let consumer = match text(row, "consumer") {
            "claude" => Consumer::Claude,
            "codex" => Consumer::Codex,
            "pi" => Consumer::Pi,
            _ => panic!("not a consumer"),
        };
        Context {
            consumer,
            home: self.home.clone(),
            user: Some("fixture-user".into()),
            cwd: self.expand(text(row, "cwd")),
            zsh_executor: consumer != Consumer::Pi,
            require_execution_owner: row["provenance_form"] == "required_execution_domain",
            shell_observation_entries: Cell::new(0),
        }
    }
    pub fn body(&self, row: &Value) -> Vec<u8> {
        if let Some(raw) = row["input"]["raw_event_bytes"].as_str() {
            return raw.as_bytes().to_vec();
        }
        let mut input = self.expand_value(&row["input"]);
        if let Some(recipe) = row["input"].get("generator") {
            let excess = usize::from(recipe["point"] == "one-unit-excess");
            let bound = recipe["bound"].as_u64().unwrap() as usize;
            let base = self.expand(recipe["base_command"].as_str().unwrap());
            let command = if recipe["kind"] == "nesting" {
                format!(
                    "{}{}{}",
                    "printf '%s' $(".repeat(bound + excess),
                    base,
                    ")".repeat(bound + excess)
                )
            } else {
                base
            };
            input = json!({"command":command});
            if recipe["kind"] == "input-bytes" {
                let event = json!({"tool_name":row["tool"],"tool_input":input});
                let length = serde_json::to_vec(&event).unwrap().len();
                input["command"] = json!(format!(
                    "{}{}",
                    input["command"].as_str().unwrap(),
                    " ".repeat(bound + excess - length)
                ));
            }
        }
        serde_json::to_vec(&json!({"tool_name":row["tool"],"tool_input":input})).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
