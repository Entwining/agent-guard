use super::*;

mod message_roles {
    #[test]
    fn commit_messages_do_not_become_pathspecs() {
        let host = crate::record::HostFacts {
            home: "/synthetic/home",
            user: None,
        };
        let observation = crate::shell::observe(
            "git commit -m 'public message'",
            crate::shell::Arm::Brush,
            host.home,
            "/synthetic/project",
            true,
        )
        .unwrap();
        let effects = super::infer(&observation.script.commands[0], "/synthetic/project", host);
        assert!(
            !effects
                .targets
                .iter()
                .any(|target| target.path.ends_with("public message"))
        );
    }
}
mod process_arguments {
    use super::*;

    fn command(argv: Vec<Word>) -> CommandRecord {
        let mut record = crate::shell::observe(
            "true",
            crate::shell::Arm::Brush,
            "/synthetic/home",
            "/synthetic/project",
            true,
        )
        .unwrap()
        .script
        .commands
        .remove(0);
        record.argv = argv;
        record
    }

    #[test]
    fn kill_unknown_width_visits_names_without_read_targets() {
        for width in [8, 16, 32, 64] {
            let mut argv = vec![Word::literal("kill".into()), Word::literal("--".into())];
            argv.extend((0..width).map(|n| {
                let mut word = Word::literal(format!("pid{n}"));
                word.cardinality_unknown = true;
                word.field_count_unknown = true;
                word.expands = true;
                word.runtime_unknown = true;
                word
            }));
            let effects = infer(
                &command(argv),
                "/synthetic/project",
                HostFacts {
                    home: "/synthetic/home",
                    user: None,
                },
            );
            assert!(!effects.gaps.contains(&CoverageGap::UnsupportedShellSyntax));
            assert_eq!(effects.targets.len(), width + 1);
            assert!(
                effects
                    .targets
                    .iter()
                    .all(|target| target.effect == Effect::Name)
            );
            assert_eq!(effects.owner_visits, 1);
            assert_eq!(effects.argv_words, width + 2);
        }
    }

    #[test]
    fn perl_argv_forwarding_visits_each_owner_once() {
        for width in [2, 4, 8, 16] {
            for depth in [1, 2, 4, 8] {
                let mut argv = Vec::new();
                for _ in 0..depth {
                    argv.extend(
                        ["perl", "-e", "alarm 10; exec @ARGV"].map(|v| Word::literal(v.into())),
                    );
                }
                argv.push(Word::literal("kill".into()));
                argv.extend((0..width).map(|n| Word::literal(format!("pid{n}"))));
                let effects = infer(
                    &command(argv),
                    "/synthetic/project",
                    HostFacts {
                        home: "/synthetic/home",
                        user: None,
                    },
                );
                assert!(!effects.gaps.contains(&CoverageGap::InterpreterChosenRead));
                assert_eq!(effects.owner_visits, depth + 1);
                assert!(effects.argv_words <= (depth + 1) * (width + 3 * depth + 1));
                assert_eq!(
                    effects
                        .targets
                        .iter()
                        .filter(|target| target.effect == Effect::Name)
                        .count(),
                    width
                );
                assert!(effects.targets.len() <= width + depth + 1);
            }
        }
    }
}

mod record_tests {
    use super::*;
    #[test]
    fn control_flow_builtins_are_not_unmodelled_programs() {
        let host = HostFacts {
            home: "/h",
            user: None,
        };
        for source in ["break", "continue", "return", "D=public true"] {
            let observation =
                crate::shell::observe(source, crate::shell::Arm::Brush, host.home, "/p", true)
                    .unwrap();
            let command = observation
                .script
                .commands
                .iter()
                .find(|c| c.program.is_some())
                .unwrap();
            let effects = infer(command, &command.cwd, host);
            if source == "D=public true" {
                assert!(effects.targets.is_empty(), "{source}: {effects:?}");
            } else {
                assert_eq!(effects.targets.len(), 1, "{source}: {effects:?}");
                let target = &effects.targets[0];
                assert_eq!(target.path, command.cwd);
                assert_eq!(target.effect, Effect::Enter);
                assert_eq!(target.via, Via::Cwd);
                assert_eq!(target.walk, Walk::None);
            }
            assert!(effects.gaps.is_empty(), "{source}: {effects:?}");
        }
    }
    #[test]
    fn compound_use_records_have_the_use_effect() {
        let packet: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/rust-m2-scopes.json")).unwrap();
        let host = HostFacts {
            home: "/h",
            user: None,
        };
        let rows: Vec<_> = packet["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["partition"] == "use")
            .collect();
        assert!(!rows.is_empty(), "missing compound use partition");
        for row in rows {
            let observation = crate::shell::observe(
                row["source"].as_str().unwrap(),
                crate::shell::Arm::Brush,
                host.home,
                "/h/p",
                true,
            )
            .unwrap();
            let command = observation
                .script
                .commands
                .iter()
                .find(|command| command.program.is_none() && !command.argv.is_empty())
                .unwrap();
            let effects = infer(command, &command.cwd, host);
            assert_eq!(effects.targets.len(), 1);
            assert_eq!(effects.targets[0].effect, Effect::Use);
            assert_eq!(effects.targets[0].unresolved, command.argv[0].text);
        }
        let row = packet["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == "assignment-data-control")
            .unwrap();
        let observation = crate::shell::observe(
            row["source"].as_str().unwrap(),
            crate::shell::Arm::Brush,
            host.home,
            "/h/p",
            true,
        )
        .unwrap();
        let assignment = observation
            .script
            .commands
            .iter()
            .find(|command| command.program.is_none() && !command.argv.is_empty())
            .unwrap();
        assert!(infer(assignment, &assignment.cwd, host).targets.is_empty());
    }
    fn targets(source: &str) -> Vec<Target> {
        let host = HostFacts {
            home: "/h",
            user: Some("fixture-user"),
        };
        let script = crate::shell::observe_with_user(
            source,
            crate::shell::Arm::Brush,
            host.home,
            "/p",
            host.user,
            false,
        )
        .unwrap()
        .script;
        infer(&script.commands[0], "/p", host).targets
    }
    #[test]
    fn ls_records_list_effect() {
        for source in ["ls .env", "ls -R public"] {
            let targets = targets(source);
            assert_eq!(targets.len(), 1);
            assert_eq!(
                targets[0].path,
                if source == "ls .env" {
                    "/p/.env"
                } else {
                    "/p/public"
                }
            );
            assert!(targets.iter().all(|target| target.effect == Effect::List));
        }
    }
    #[test]
    fn search_flag_has_an_explicit_owner() {
        assert!(targets("rg needle")[0].search);
        assert!(!targets("rg needle public")[0].search);
        assert!(!targets("rg --help")[0].search);
        assert!(!targets("git log -p public")[0].search);
    }

    #[test]
    fn p7_role_dependencies_follow_go_owners() {
        let packet: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/rust-m2-filesystem.json"))
                .unwrap();
        let rows: Vec<_> = packet["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["role"].is_string())
            .collect();
        assert!(!rows.is_empty(), "missing P7 role dependency partition");
        for row in rows {
            let target = targets(row["source"].as_str().unwrap())
                .into_iter()
                .find(|t| t.path.starts_with("/h/Library/Containers"))
                .unwrap();
            assert_eq!(
                format!("{:?}", target.effect),
                row["role"].as_str().unwrap(),
                "{}: {}",
                row["id"],
                row["owner"]
            );
        }
    }
}
