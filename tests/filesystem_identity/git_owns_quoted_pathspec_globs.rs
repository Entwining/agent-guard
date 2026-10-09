#[path = "../support/cases/git_owns_quoted_pathspec_globs.rs"]
pub mod cases;
use cases::*;

#[test]
fn file_urls_reach_protected_identity() {
    regressions("file_url");
    let fixture = support::Fixture::new();
    // Pi is the tool consumer that decodes a `file://` path.
    let context = fixture.context(&json!({"consumer":"pi","cwd":"$P"}));
    let bytes = serde_json::to_vec(&json!({"tool_name":"read","tool_input":{
        "path":format!("file://{}/Library/Containers/App/data",fixture.home)
    }}))
    .unwrap();
    let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
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

#[test]
fn structured_targets_do_not_infer_shell_globs() {
    let target = Target::new("/p/.e[n]v".into(), Effect::Read, Walk::Visible, Via::Tool);
    assert!(!target.glob && !target.search);
    let fixture = support::Fixture::new();
    let context = fixture.context(&json!({"consumer":"claude","cwd":"$P"}));
    let bytes =
        serde_json::to_vec(&json!({"tool_name":"Read","tool_input":{"file_path":".e[n]v"}}))
            .unwrap();
    let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
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
        let mut probe = support::RecordingProbe::new(&fixture, word);
        let result = filesystem::identify_target(
            &target.path,
            &fixture.project,
            &fixture.home,
            false,
            patterned,
            Effect::Read,
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
