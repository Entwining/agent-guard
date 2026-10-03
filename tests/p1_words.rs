#[path = "support/differential.rs"]
mod differential;
mod support;
use agent_guard_rust::{
    record::{Direction, Stdin},
    shell::{self, Arm},
};

fn required(ids: &[&str]) {
    for &arm in shell::ACCEPTANCE_ARMS {
        for row in differential::selected_report(arm, ids) {
            assert!(row["scope"].is_null());
            for observation in row["observations"].as_array().unwrap() {
                assert_ne!(
                    observation["category"], "Rust_defect",
                    "{}: {observation}",
                    row["id"]
                );
            }
        }
    }
}

fn argv(source: &str) -> Vec<agent_guard_rust::record::Word> {
    shell::observe_with_user(
        source,
        Arm::Brush,
        "/synthetic/home",
        "/synthetic/home/project",
        Some("fixture-user"),
        true,
    )
    .unwrap()
    .script
    .commands
    .remove(0)
    .argv
}

#[test]
fn ansi_c_words() {
    let words = argv(r"cat $'\x2eenv' $'\u002eenv' $'\056env' $'a\n\t\\b'");
    for word in &words[1..4] {
        assert_eq!(word.text, ".env");
        assert!(!word.globs && !word.expands);
    }
    assert_eq!(words[4].text, "a\n\t\\b");
    required(&["credentials[266]"]);
}

#[test]
fn brace_sequence_and_quoting() {
    let words = argv(
        r#"cat ~/.e{n..n}v "$HOME/Library/{Containers,CloudStorage}" "$HOME/Library/*" '$HOME/.env'"#,
    );
    assert_eq!(words[1].text, "/synthetic/home/.e*v");
    assert!(words[1].globs);
    assert_eq!(
        words[2].text,
        "/synthetic/home/Library/{Containers,CloudStorage}"
    );
    assert!(!words[2].globs);
    assert!(!words[3].globs && !words[3].expands);
    assert_eq!(words[4].text, "$HOME/.env");
    assert!(words[4].vars.is_empty());
    required(&[
        "options[8]",
        "options[9]",
        "options[10]",
        "options[11]",
        "appdata[83]",
    ]);
}

#[test]
fn user_is_a_host_fact() {
    let words = argv("ls ~fixture-user/Library/Containers ~foreign-user/public");
    assert_eq!(words[1].text, "/synthetic/home/Library/Containers");
    assert_eq!(words[2].text, "~foreign-user/public");
    let absent = shell::observe_with_user(
        "echo ~unknown ~fixture-user",
        Arm::Brush,
        "/h",
        "/p",
        None,
        false,
    )
    .unwrap();
    assert_eq!(absent.script.commands[0].argv[1].text, "/h");
    assert_eq!(absent.script.commands[0].argv[2].text, "~fixture-user");
    let rebound = argv("USER=foreign-user; ls ~fixture-user/Library/Containers");
    assert_eq!(rebound[1].text, "/synthetic/home/Library/Containers");
    required(&["appdata[85]"]);
}

#[test]
fn pwd_word_semantics() {
    let words = argv("cat ~+/x $(pwd)/x `pwd -P`/x $PWD/x");
    for word in &words[1..] {
        assert_eq!(word.text, "/synthetic/home/project/x");
        assert!(word.pwd && !word.expands);
    }
    required(&["cwd[39]", "cwd[40]", "cwd[41]"]);
}

#[test]
fn unresolved_word_fragments() {
    let words = argv(
        r#"ls "$ROOT/Library/Containers" "${D:+~/Library/Containers/x}" "${value:-$API_KEY}""#,
    );
    assert_eq!(words[1].text, "$ROOT/Library/Containers");
    assert!(words[1].expands);
    assert_eq!(words[1].vars, ["ROOT"]);
    assert!(words[2].expands);
    assert_eq!(words[3].vars, ["value", "API_KEY"]);
    let children = argv(
        r#"echo $((1+$COUNT)) "${v/$PATTERN/$REPLACEMENT}" "${v:$OFFSET:$LENGTH}" "${a[$INDEX]}" "$@" "$1""#,
    );
    assert_eq!(children[1].vars, ["COUNT"]);
    assert_eq!(children[2].vars, ["v", "PATTERN", "REPLACEMENT"]);
    assert_eq!(children[3].vars, ["v", "OFFSET", "LENGTH"]);
    assert_eq!(children[4].vars, ["a", "INDEX"]);
    assert_eq!(children[5].vars, ["@"]);
    assert_eq!(children[6].vars, ["1"]);
    required(&["appdata[48]", "appdata[132]", "cwd[15]", "credentials[135]"]);
}

#[test]
fn word_piece_record_transport() {
    let words = argv("cat public\\ file \"\\\n.env\"");
    assert_eq!(words[1].text, "public file");
    assert_eq!(words[1].value, "public file");
    assert_eq!(words[1].raw, "public\\ file");
    assert_eq!(words[2].text, ".env");
    let observation = shell::observe(
        "cat < '$HOME/*.pub' <<'EOF'\nliteral '$HOME'\nEOF",
        Arm::Brush,
        "/h",
        "/p",
        false,
    )
    .unwrap();
    let command = &observation.script.commands[0];
    assert_eq!(command.redirects[0].direction, Direction::In);
    assert!(!command.redirects[0].globs);
    assert_eq!(command.redirects[1].direction, Direction::Heredoc);
    assert_eq!(command.redirects[1].target, "literal '$HOME'\n");
    assert!(command.redirects[1].vars.is_empty());
    assert_eq!(command.stdin, Stdin::Data(vec![1]));
    let fixture = support::Fixture::new();
    let context = fixture.context(&serde_json::json!({"consumer":"claude","cwd":"$P"}));
    for (source, expected) in [
        ("fd --search-path \"$ROOT/Library/Containers\"", "D"),
        ("fd --search-path 'public*'", "N"),
    ] {
        let bytes = serde_json::to_vec(
            &serde_json::json!({"tool_name":"Bash","tool_input":{"command":source}}),
        )
        .unwrap();
        let mut probe = support::RecordingProbe::new(&fixture);
        let result = agent_guard_rust::evaluate_with_arm(
            agent_guard_rust::Event {
                bytes: &bytes,
                context: &context,
                probe: &mut probe,
            },
            Arm::Brush,
        );
        assert_eq!(support::class(&result), expected);
    }
    assert!(
        shell::observe("if then", Arm::Brush, "/h", "/p", false)
            .unwrap()
            .script
            .parse_failed
    );
    required(&["appdata[49]", "appdata[50]", "shell[20]", "shell[44]"]);
}

#[test]
fn parser_acceptance_denominator() {
    required(&["credentials[266]", "options[8]", "appdata[85]"]);
    assert_eq!(shell::ACCEPTANCE_ARMS, &[Arm::Brush]);
}
