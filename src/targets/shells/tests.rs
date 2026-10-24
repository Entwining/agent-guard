use super::*;

fn invocation(command: &str) -> ShellInvocation {
    let mut observation = crate::shell::observe(
        command,
        crate::shell::Arm::Brush,
        "/synthetic/home",
        "/synthetic/project",
        true,
    )
    .unwrap();
    let record = observation.script.commands.remove(0);
    let index = record.program.unwrap();
    shell_invocation(
        record.argv[index].rsplit('/').next().unwrap(),
        &record.argv[index + 1..],
    )
    .unwrap()
}

#[test]
fn option_values_and_terminators_place_the_source() {
    for (command, source, operands) in [
        (
            "bash -c -O extglob 'cat .env'",
            ShellSource::Code(Some(3)),
            3,
        ),
        (
            "bash -O extglob -c 'cat .env'",
            ShellSource::Code(Some(3)),
            3,
        ),
        ("bash -cO extglob 'cat .env'", ShellSource::Code(Some(2)), 2),
        ("bash -c -- 'cat .env'", ShellSource::Code(Some(2)), 2),
        ("bash -x -O extglob 'cat .env'", ShellSource::Script(3), 3),
        ("bash -s -x .env", ShellSource::Stdin, 2),
        ("bash -- -x", ShellSource::Script(1), 1),
        (
            "zsh -oextendedglob -c 'cat .env'",
            ShellSource::Code(Some(2)),
            2,
        ),
        (
            "zsh -c -o extendedglob 'cat .env'",
            ShellSource::Code(Some(3)),
            3,
        ),
        ("zsh -x- -c", ShellSource::Script(1), 1),
        ("ksh - -c 'cat .env'", ShellSource::Code(Some(2)), 2),
        ("tcsh -c 'cat .env' -x .env", ShellSource::Code(Some(1)), 3),
        ("tcsh -b -c", ShellSource::Script(1), 1),
    ] {
        let invocation = invocation(command);
        assert_eq!(invocation.source, source, "{command}");
        assert_eq!(invocation.operands, operands, "{command}");
    }
}

#[test]
fn consumed_option_values_keep_their_resource_effect() {
    for (command, values) in [
        ("bash --rcfile .env -ic true", vec![(1, Some(Effect::Read))]),
        (
            "bash --init-file .env -ic true",
            vec![(1, Some(Effect::Read))],
        ),
        (
            "bash -O extglob -o pipefail -c true",
            vec![(1, None), (3, None)],
        ),
        ("ksh -R xref.db script", vec![(1, Some(Effect::Write))]),
        ("zsh -o extendedglob -c true", vec![(1, None)]),
    ] {
        assert_eq!(invocation(command).values, values, "{command}");
    }
}

#[test]
fn an_unplaced_word_keeps_the_known_code_mode() {
    let invocation = invocation("bash -c -Q 'cat .env'");
    assert_eq!(invocation.source, ShellSource::Uncertain(vec![2]));
    assert_eq!(invocation.operands, 1);
}

#[test]
fn consecutive_unplaced_words_keep_every_later_source_in_linear_steps() {
    for width in [1, 2, 4, 8, 16] {
        let mut command = "bash".to_owned();
        for _ in 0..width {
            command.push_str(" \"${MODE:--x}\"");
        }
        command.push_str(" 'cat .env; true'");
        let invocation = invocation(&command);
        let ShellSource::Uncertain(code) = &invocation.source else {
            panic!("{command}");
        };
        assert!(code.contains(&width), "{command}");
        assert_eq!(invocation.operands, 0);
        // A reading's state is one of two modes, two stdin choices and three
        // expectations after an unplaced word.
        assert!(invocation.steps <= 12 * (width + 1), "{command}");
    }
}
