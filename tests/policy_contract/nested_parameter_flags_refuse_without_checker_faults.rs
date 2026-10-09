#[path = "../support/cases/nested_parameter_flags_refuse_without_checker_faults.rs"]
pub mod cases;
use cases::*;

#[test]
fn probe_fault_stays_fault_with_independently_observed_denial() {
    use agent_guard_rust::{CheckErrorKind, shell};
    struct Fault;
    impl agent_guard_rust::filesystem::Probe for Fault {
        fn stat(
            &mut self,
            _: &std::path::Path,
        ) -> std::io::Result<Option<agent_guard_rust::filesystem::Metadata>> {
            Err(std::io::ErrorKind::PermissionDenied.into())
        }
        fn read_link(
            &mut self,
            _: &std::path::Path,
        ) -> std::io::Result<Option<std::path::PathBuf>> {
            Err(std::io::ErrorKind::PermissionDenied.into())
        }
    }
    let source = "cat .env; echo ${v:-${(f)v}}; cat public";
    let observation = shell::observe(
        source,
        Arm::Brush,
        "/synthetic/home",
        "/synthetic/home/project",
        true,
    )
    .unwrap();
    assert!(
        observation
            .script
            .commands
            .iter()
            .any(|c| c.argv.iter().any(|w| w.text == ".env")),
        "independent protected operand survives refusal"
    );
    let context = agent_guard_rust::Context {
        consumer: adapters::Consumer::Claude,
        home: "/synthetic/home".into(),
        cwd: "/synthetic/home/project".into(),
        user: None,
        zsh_executor: true,
        require_execution_owner: false,
        shell_observation_entries: std::cell::Cell::new(0),
    };
    let bytes =
        serde_json::to_vec(&json!({"tool_name":"Bash","tool_input":{"command":source}})).unwrap();
    let result = evaluate_with_arm(
        Event {
            bytes: &bytes,
            context: &context,
            probe: &mut Fault,
        },
        Arm::Brush,
    );
    assert_eq!(result.unwrap_err().kind, CheckErrorKind::ProbeFault);
}
