use agent_guard_rust::{CheckErrorKind, Event, Outcome, adapters, evaluate, filesystem};
use serde_json::json;
use std::path::{Path, PathBuf};

const SOURCE: &str = "cat .env; echo ${v:-${(f)v}}; cat public";

/// Every path is missing except `faulty`, whose probe fails.
struct FaultAt(Option<&'static str>);

impl FaultAt {
    fn check(&self, path: &Path) -> std::io::Result<()> {
        match self.0 {
            Some(name) if path.file_name().is_some_and(|file| file == name) => {
                Err(std::io::ErrorKind::PermissionDenied.into())
            }
            _ => Ok(()),
        }
    }
}

impl filesystem::Probe for FaultAt {
    fn stat(&mut self, path: &Path) -> std::io::Result<Option<filesystem::Metadata>> {
        self.check(path).map(|()| None)
    }
    fn read_link(&mut self, path: &Path) -> std::io::Result<Option<PathBuf>> {
        self.check(path).map(|()| None)
    }
}

fn evaluate_with(
    probe: &mut FaultAt,
) -> Result<agent_guard_rust::Evaluation, agent_guard_rust::CheckError> {
    let context = agent_guard_rust::Context {
        consumer: adapters::Consumer::Claude,
        home: "/synthetic/home".into(),
        cwd: "/synthetic/home/project".into(),
        user: None,
        zsh_executor: true,
        config_dirs: Vec::new(),
        shell_observation_entries: std::cell::Cell::new(0),
    };
    let bytes =
        serde_json::to_vec(&json!({"tool_name":"Bash","tool_input":{"command":SOURCE}})).unwrap();
    evaluate(Event {
        bytes: &bytes,
        context: &context,
        probe,
    })
}

#[test]
fn probe_fault_on_a_later_operand_outranks_an_earlier_denial() {
    let result = evaluate_with(&mut FaultAt(Some("public")));
    assert_eq!(result.unwrap_err().kind, CheckErrorKind::ProbeFault);
}

#[test]
fn the_same_call_denies_when_every_probe_succeeds() {
    let evaluation = evaluate_with(&mut FaultAt(None)).unwrap();
    assert!(
        matches!(evaluation.outcome, Outcome::ProtectedDenial { .. }),
        "{:?}",
        evaluation.outcome
    );
}
