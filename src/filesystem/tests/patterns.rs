#[test]
fn equal_cooked_paths_with_different_quotes_keep_distinct_identities() {
    struct NoLinks;
    impl super::super::Probe for NoLinks {
        fn read_link(
            &mut self,
            _: &std::path::Path,
        ) -> std::io::Result<Option<std::path::PathBuf>> {
            Ok(None)
        }
        fn stat(&mut self, _: &std::path::Path) -> std::io::Result<Option<super::super::Metadata>> {
            Ok(None)
        }
    }
    let packet: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust-batch17e-patterns.json"
    ))
    .unwrap();
    let rows = packet["rows"].as_array().unwrap();
    let targets: Vec<_> = ["same-cooked-quoted-brace", "same-cooked-unquoted-brace"]
        .iter()
        .map(|id| {
            let row = rows.iter().find(|row| row["id"] == *id).unwrap();
            let observation = crate::shell::observe(
                row["input"]["command"].as_str().unwrap(),
                crate::shell::Arm::Brush,
                "/h",
                "/p",
                true,
            )
            .unwrap();
            crate::record::Target::from_word(
                &observation.script.commands.last().unwrap().argv[1],
                "/p",
                crate::record::HostFacts {
                    home: "/h",
                    user: None,
                },
                crate::record::Effect::Read,
                crate::record::Walk::None,
            )
        })
        .collect();
    assert_eq!(targets[0].unresolved, targets[1].unresolved);
    for order in [[0, 1], [1, 0]] {
        let table = super::super::FirmlinkTable::from_text("");
        let mut resolver = super::super::Resolver::new("/h", &table);
        for index in order {
            let identity = resolver
                .target(&mut targets[index].clone(), "/p", &mut NoLinks)
                .unwrap();
            assert_eq!(
                matches!(
                    identity,
                    super::super::Identity::Protected(super::super::Protection::Environment)
                ),
                index == 1
            );
        }
    }
}
#[test]
fn quoted_pattern_width_does_not_multiply_local_candidates() {
    for width in [2, 4, 8, 16] {
        for depth in [1, 2, 4, 8] {
            let groups = format!("({})", vec!["public"; width].join("|"));
            let source = format!("cat 'public{}'*.txt", groups.repeat(depth));
            let observation = crate::shell::observe(
                &source,
                crate::shell::Arm::Brush,
                "/synthetic/home",
                "/synthetic/project",
                true,
            )
            .unwrap();
            let word = &observation.script.commands.last().unwrap().argv[1];
            let target = crate::record::Target::from_word(
                word,
                "/synthetic/project",
                crate::record::HostFacts {
                    home: "/synthetic/home",
                    user: None,
                },
                crate::record::Effect::Read,
                crate::record::Walk::None,
            );
            assert!(target.glob);
            let pattern = target.pattern_path();
            let candidates = super::super::glob::alternatives(&pattern, true).unwrap();
            assert_eq!(candidates.len(), 1, "width={width}, depth={depth}");
            let mut matcher = super::super::glob::Matcher::default();
            for candidate in candidates {
                assert_eq!(
                    super::super::lexical_candidate(
                        &candidate,
                        &super::super::lexical::Domain::new("/synthetic/home"),
                        true,
                        false,
                        &mut matcher
                    ),
                    None
                );
            }
            assert!(
                matcher.component_queries <= 256,
                "width={width}, depth={depth}, queries={}",
                matcher.component_queries
            );
            println!(
                "quoted width={width}, depth={depth}, queries={}",
                matcher.component_queries
            );
        }
    }
}
#[test]
fn repeated_pattern_components_share_match_work() {
    for size in [8, 16, 32] {
        let mut matcher = super::super::glob::Matcher::default();
        let mut first = None;
        for _ in 0..size {
            assert_eq!(
                super::super::lexical_candidate(
                    "/h/project/public[a-z]/nested/public.json",
                    &super::super::lexical::Domain::new("/h"),
                    true,
                    false,
                    &mut matcher
                ),
                None
            );
            assert_eq!(
                matcher.component_evaluations,
                *first.get_or_insert(matcher.component_evaluations),
                "size={size}"
            );
        }
        assert!(matcher.component_evaluations > 0);
    }
}
#[test]
fn repeated_resource_identity_shares_probe_work() {
    struct Counted {
        links: usize,
        stats: usize,
    }
    impl super::super::Probe for Counted {
        fn read_link(
            &mut self,
            path: &std::path::Path,
        ) -> std::io::Result<Option<std::path::PathBuf>> {
            self.links += 1;
            Ok((path == std::path::Path::new("/p/link")).then(|| "/public".into()))
        }
        fn stat(&mut self, _: &std::path::Path) -> std::io::Result<Option<super::super::Metadata>> {
            self.stats += 1;
            Ok(None)
        }
    }
    use crate::record::{Effect, Target, Via, Walk};
    let table = super::super::FirmlinkTable::from_text("");
    for size in [8, 16, 32] {
        let mut resolver = super::super::Resolver::new("/h", &table);
        let mut probe = Counted { links: 0, stats: 0 };
        let mut first = None;
        for _ in 0..size {
            let mut target =
                Target::new("/p/link/x".into(), Effect::Read, Walk::None, Via::Operand);
            assert_eq!(
                resolver.target(&mut target, "/p", &mut probe).unwrap(),
                super::super::Identity::Public("/public/x".into())
            );
            assert_eq!(
                (&target.path, &target.unresolved),
                (&"/public/x".to_owned(), &"/public/x".to_owned())
            );
            let work = (probe.links, probe.stats);
            assert_eq!(work, *first.get_or_insert(work), "size={size}");
            assert_eq!(
                resolver.home(&mut probe).unwrap(),
                super::super::Identity::Public("/h".into())
            );
            assert_eq!(
                (probe.links, probe.stats),
                work,
                "repeated HOME: size={size}"
            );
        }
        assert!(probe.links > 0);
        let mut root = Target::new("/h/.aws".into(), Effect::List, Walk::None, Via::Operand);
        assert!(matches!(
            resolver.target(&mut root, "/p", &mut probe).unwrap(),
            super::super::Identity::Public(_)
        ));
        root.effect = Effect::Read;
        assert_eq!(
            resolver.target(&mut root, "/p", &mut probe).unwrap(),
            super::super::Identity::Protected(super::super::Protection::Credential)
        );
    }
}
#[test]
fn protected_ssh_candidate_is_a_policy_result_before_stat() {
    struct NoStat {
        calls: usize,
    }
    impl super::super::Probe for NoStat {
        fn read_link(
            &mut self,
            _: &std::path::Path,
        ) -> std::io::Result<Option<std::path::PathBuf>> {
            Ok(None)
        }
        fn stat(&mut self, _: &std::path::Path) -> std::io::Result<Option<super::super::Metadata>> {
            self.calls += 1;
            Err(std::io::Error::other("unexpected identity stat"))
        }
    }
    let rows: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust-public-ssh-backups.json"
    ))
    .unwrap();
    let candidate = rows["protected_candidate"].as_str().unwrap();
    let mut probe = NoStat { calls: 0 };
    let table = super::super::FirmlinkTable::from_text("");
    let result = super::super::Resolver::new("/synthetic/home", &table)
        .ssh_denied(
            candidate,
            candidate,
            "/synthetic/project",
            "/synthetic/home",
            false,
            &mut probe,
        )
        .unwrap();
    assert_eq!(result, Some(true));
    assert_eq!(probe.calls, 0);
}
#[test]
fn lexical_candidates_observe_the_evaluation_deadline() {
    let expired = std::time::Instant::now() - std::time::Duration::from_secs(1);
    assert_eq!(
        super::super::lexical_pattern_checked(
            "public",
            "/synthetic/home",
            true,
            true,
            Some(expired)
        )
        .unwrap_err()
        .kind,
        crate::CheckErrorKind::Deadline
    );
}
#[test]
fn literal_api_root_does_not_expand_into_a_hidden_directory() {
    let path = format!("{}/credentials.ts", super::super::literal_glob_root("a/*"));
    assert_eq!(
        super::super::lexical_pattern_mode(&path, "/h", true, true),
        None
    );
    assert_eq!(
        super::super::lexical_pattern_mode("a/*/credentials.ts", "/h", true, true),
        Some(super::super::Protection::Credential)
    );
}
