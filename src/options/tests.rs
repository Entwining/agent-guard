use super::*;
use crate::record::Effect::{Name, Read, Write};

const GETOPT: Grammar = Grammar {
    syntax: Syntax::Getopt,
    short: "nr",
    flags: "--reverse",
    values: &[("-o --output", Write), ("-f", Read), ("-k", Name)],
    attached: &[("-i --in-place", Name)],
    stops: false,
    complete: true,
};

/// The arguments after the program in `command`, as the shell records them.
fn args(command: &str) -> Vec<Word> {
    let observation = crate::shell::observe(
        command,
        crate::shell::Arm::Brush,
        "/synthetic/home",
        "/synthetic/project",
        true,
    )
    .unwrap();
    observation.script.commands[0].argv[1..].to_vec()
}

/// Each bound value's name and text, and the binding.
fn bound(command: &str, grammar: &Grammar) -> (Vec<(&'static str, String)>, Binding) {
    let args = args(command);
    let binding = bind(&args, grammar);
    let values = binding
        .values
        .iter()
        .map(|bound| (bound.name, bound.value.word(&args).text))
        .collect();
    (values, binding)
}

#[test]
fn glued_grouped_and_separate_values_bind_the_same_value() {
    for command in [
        "p -o.env in",
        "p -o .env in",
        "p -nro.env in",
        "p -nro .env in",
        "p --output=.env in",
        "p --output .env in",
    ] {
        let (values, binding) = bound(command, &GETOPT);
        let name = if command.contains("--") {
            "--output"
        } else {
            "-o"
        };
        assert_eq!(values, [(name, ".env".to_owned())], "{command}");
        assert_eq!(binding.unproven, None, "{command}");
        assert_eq!(binding.operands.len(), 1, "{command}");
    }
}

#[test]
fn only_clap_drops_the_equals_sign_of_a_short_value() {
    let (getopt, _) = bound("p -o=out", &GETOPT);
    assert_eq!(getopt, [("-o", "=out".to_owned())]);
    let clap = Grammar {
        syntax: Syntax::Clap,
        ..GETOPT
    };
    let (clap, _) = bound("p -o=out", &clap);
    assert_eq!(clap, [("-o", "out".to_owned())]);
}

#[test]
fn an_attached_value_never_takes_the_next_word() {
    let (values, binding) = bound("p -i .bak -i.orig --in-place in", &GETOPT);
    assert_eq!(values, [("-i", ".orig".to_owned())]);
    assert!(binding.has("-i") && binding.has("--in-place"));
    assert_eq!(binding.operands, [1, 4]);
}

#[test]
fn a_stopping_grammar_reads_options_after_an_operand_as_operands() {
    let stops = Grammar {
        stops: true,
        ..GETOPT
    };
    let (values, binding) = bound("p in -f .env", &stops);
    assert!(values.is_empty());
    assert_eq!(binding.operands, [0, 1, 2]);
    let (values, _) = bound("p in -f .env", &GETOPT);
    assert_eq!(values, [("-f", ".env".to_owned())]);
}

#[test]
fn an_unlisted_option_leaves_a_complete_grammar_unproven() {
    let (_, binding) = bound("p -x -f .env", &GETOPT);
    assert_eq!(binding.unproven, Some(0));
    let partial = Grammar {
        complete: false,
        ..GETOPT
    };
    let (values, binding) = bound("p -xf .env --unknown=v", &partial);
    assert_eq!(values, [("-f", ".env".to_owned())]);
    assert_eq!(binding.unproven, None);
    // The program's generic roles keep a word no option claims.
    assert_eq!(binding.options, [0, 1]);
}

#[test]
fn runtime_text_proves_no_role_unless_a_known_option_already_holds_it() {
    let (values, binding) = bound(r#"p -f.env"$X" in"#, &GETOPT);
    assert_eq!(values.len(), 1);
    assert_eq!(binding.unproven, None);
    // The runtime text may be empty, leaving the value to the next word.
    let (values, binding) = bound(r#"p -f"$X" in"#, &GETOPT);
    assert_eq!(values.len(), 1);
    assert_eq!(binding.unproven, Some(0));
    let (_, binding) = bound(r#"p "$X" in"#, &GETOPT);
    assert_eq!(binding.unproven, Some(0));
    assert_eq!(binding.operands, [0, 1]);
}
