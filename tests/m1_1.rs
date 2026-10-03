mod support;
use agent_guard_rust::{
    Event, evaluate_with_arm,
    filesystem::{self, Probe},
    record::{Direction, Effect, HostFacts, Target, Via, Walk},
    shell::{self, Arm},
};
use serde_json::{Value, json};
use std::{panic::AssertUnwindSafe, path::Path};

fn regressions(owner: &str) {
    let rows: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/rust-m1-1-regressions.json")).unwrap();
    let fixture = support::Fixture::new();
    for consumer in ["claude", "codex", "pi"] {
        let context = fixture.context(&json!({"consumer":consumer,"cwd":"$P"}));
        for row in rows.iter().filter(|r| r["owner"] == owner) {
            let body = serde_json::to_vec(&json!({
                "tool_name":if consumer == "pi" {"bash"} else {"Bash"},
                "tool_input":{"command":fixture.expand(row["source"].as_str().unwrap())}
            }))
            .unwrap();
            let mut probe = support::RecordingProbe::new(&fixture);
            let result = evaluate_with_arm(
                Event {
                    bytes: &body,
                    context: &context,
                    probe: &mut probe,
                },
                Arm::Brush,
            );
            assert_eq!(
                support::class(&result),
                row["expected"],
                "{consumer}: {row}"
            );
        }
    }
}

#[test]
fn git_owns_quoted_pathspec_globs() {
    regressions("git");
}
#[test]
fn brace_union_and_nested_lists() {
    regressions("brace");
}
#[test]
fn file_urls_reach_protected_identity() {
    regressions("file_url");
    let fixture = support::Fixture::new();
    let context = fixture.context(&json!({"consumer":"claude","cwd":"$P"}));
    for prefix in ["file://", "FiLe://"] {
        let bytes = serde_json::to_vec(&json!({"tool_name":"Read","tool_input":{
            "file_path":format!("{prefix}{}/Library/Containers/App/data",fixture.home)
        }}))
        .unwrap();
        let mut probe = support::RecordingProbe::new(&fixture);
        let result = evaluate_with_arm(
            Event {
                bytes: &bytes,
                context: &context,
                probe: &mut probe,
            },
            Arm::Brush,
        );
        assert_eq!(support::class(&result), "D");
        assert!(probe.calls.is_empty() && probe.stat_calls.is_empty());
    }
}
#[test]
fn quoted_parentheses_stay_literal() {
    regressions("paren");
}
#[test]
fn redirect_variables_follow_direction() {
    regressions("redirect");
    let observation = shell::observe(
        "cat < $SECRET > $SECRET <<< $SECRET",
        Arm::Brush,
        "/h",
        "/p",
        false,
    )
    .unwrap();
    let redirects = &observation.script.commands[0].redirects;
    for redirect in &redirects[..2] {
        assert!(matches!(redirect.direction, Direction::In | Direction::Out));
        assert!(redirect.vars.is_empty());
        assert!(redirect.expands);
    }
    assert_eq!(redirects[2].vars, ["SECRET"]);
}
#[test]
fn structured_targets_do_not_infer_shell_globs() {
    let target = Target::new("/p/.e[n]v".into(), Effect::Read, Walk::Visible, Via::Tool);
    assert!(!target.glob && !target.search);
    let fixture = support::Fixture::new();
    let context = fixture.context(&json!({"consumer":"claude","cwd":"$P"}));
    let bytes =
        serde_json::to_vec(&json!({"tool_name":"Read","tool_input":{"file_path":".e[n]v"}}))
            .unwrap();
    let mut probe = support::RecordingProbe::new(&fixture);
    assert_eq!(
        support::class(&evaluate_with_arm(
            Event {
                bytes: &bytes,
                context: &context,
                probe: &mut probe
            },
            Arm::Brush
        )),
        "N"
    );
}
#[test]
fn glob_probe_oracle_uses_word_provenance() {
    let fixture = support::Fixture::new();
    let host = HostFacts {
        home: &fixture.home,
        user: Some("fixture-user"),
    };
    for (source, patterned) in [
        ("cat $HOME/Library/{Containers,CloudStorage}/x", true),
        ("cat \"$HOME/Library/{Containers,CloudStorage}/x\"", false),
    ] {
        let script = shell::observe_with_user(
            source,
            Arm::Brush,
            &fixture.home,
            &fixture.project,
            host.user,
            true,
        )
        .unwrap()
        .script;
        let word = &script.commands[0].argv[1];
        assert_eq!(word.globs, patterned);
        let target = Target::from_word(word, &fixture.project, host, Effect::Read, Walk::None);
        let mut probe = support::RecordingProbe::for_word(&fixture, word);
        let result = filesystem::identify_target(
            &target.path,
            &fixture.project,
            &fixture.home,
            false,
            patterned,
            &mut probe,
        )
        .unwrap();
        assert_eq!(
            matches!(result, filesystem::Identity::Protected(_)),
            patterned
        );
        let attempted = std::panic::catch_unwind(AssertUnwindSafe(|| {
            probe.read_link(Path::new(&target.path))
        }));
        assert_eq!(
            attempted.is_err(),
            patterned,
            "oracle must reject a protected glob before probing"
        );
    }
}
