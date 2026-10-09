use super::*;

#[test]
fn repeated_paths_and_prefixes_prepare_once_per_domain() {
    for width in [2, 4, 8, 16] {
        for depth in [1, 2, 4, 8] {
            let mut owner = Lexical::new(None);
            let mut paths = vec!["/".to_owned()];
            for _ in 0..depth {
                paths.push(format!(
                    "{}/public",
                    paths.last().unwrap().trim_end_matches('/')
                ));
            }
            for _ in 0..width {
                for path in &paths {
                    for _ in 0..4 {
                        assert_eq!(
                            owner.check(path, "/synthetic/home", false, true).unwrap(),
                            None
                        );
                    }
                }
            }
            assert_eq!(
                owner.candidate_evaluations,
                paths.len(),
                "width={width}, depth={depth}"
            );
            assert_eq!(owner.domain_preparations, 1);
            assert_eq!(owner.path_evaluations, paths.len());
            println!(
                "lexical width={width}, depth={depth}, candidates={}",
                owner.candidate_evaluations
            );
        }
    }
}

#[test]
fn overlapping_patterns_prepare_common_candidates_once() {
    for width in [2, 4, 8, 16] {
        for depth in [1, 2, 4, 8] {
            let mut owner = Lexical::new(None);
            for index in 0..width {
                let prefix = format!("/{}", vec!["public"; depth].join("/"));
                let path = format!("{prefix}/{{public{index},plain}}");
                assert_eq!(owner.check(&path, "/h", true, false).unwrap(), None);
            }
            assert_eq!(owner.candidate_evaluations, width * 2 + 1);
            assert_eq!(owner.domain_preparations, 1);
            println!(
                "overlap width={width}, depth={depth}, candidates={}",
                owner.candidate_evaluations
            );
        }
    }
}

#[test]
fn distinct_paths_reuse_components_and_bound_cache_storage() {
    for width in [2, 4, 8, 16] {
        for depth in [1, 2, 4, 8] {
            let mut owner = Lexical::new(None);
            for index in 0..width {
                let path = format!(
                    "/h/{}/public{index}*/report*.txt",
                    vec!["shared"; depth].join("/")
                );
                assert_eq!(owner.check(&path, "/h", true, true).unwrap(), None);
                let misses = owner.matcher.component_evaluations;
                assert_eq!(owner.check(&path, "/h", true, false).unwrap(), None);
                assert_eq!(owner.matcher.component_evaluations, misses);
                assert!(misses > 0);
            }
            let domain = &owner.domains["/h"];
            assert_eq!(domain.paths.len(), width);
            assert_eq!(domain.candidates.len(), width);
            assert_eq!(owner.candidate_evaluations, width * 2);
            let bytes: usize = domain
                .paths
                .keys()
                .chain(domain.candidates.keys())
                .map(String::len)
                .sum();
            assert!(bytes <= width * (depth * 7 + 24) * 2);
            println!(
                "unique width={width}, depth={depth}, candidates={}, key_bytes={bytes}, component_misses={}",
                owner.candidate_evaluations, owner.matcher.component_evaluations
            );
        }
    }
}

#[test]
fn cache_domains_preserve_spelling_visibility_and_deadlines() {
    let packet: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust-batch17e-patterns.json"
    ))
    .unwrap();
    let path = packet["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "ssh-remote-unquoted-glob")
        .unwrap()["input"]["command"]
        .as_str()
        .unwrap()
        .strip_prefix("ssh audit.invalid ")
        .unwrap()
        .replace("$H", "/h");
    let mut owner = Lexical::new(None);
    for home in ["/other", "/h"] {
        assert_eq!(
            owner.check(&path, home, true, false).unwrap(),
            (home == "/h").then_some(Protection::AppData)
        );
        assert_eq!(
            owner.check(&path, home, false, false).unwrap().is_some(),
            home == "/h"
        );
        assert!(owner.broad(home, home, false).unwrap());
        assert!(!owner.broad("/project", home, false).unwrap());
    }
    assert_eq!(owner.domain_preparations, 2);
    // Results for hidden candidates and exact SSH spellings cannot leak
    // into the other visibility or case domain.
    assert_eq!(owner.check("/p/*rc", "/h", true, false).unwrap(), None);
    assert_eq!(
        owner.check("/p/*rc", "/h", true, true).unwrap(),
        Some(Protection::Credential)
    );
    assert_eq!(owner.check("/p/*rc", "/h", false, true).unwrap(), None);
    for (name, expected) in [("config", None), ("CONFIG", Some(Protection::SshPrivate))] {
        assert_eq!(
            owner
                .check(&format!("/h/.ssh/{name}"), "/h", false, true)
                .unwrap(),
            expected
        );
    }
    owner.deadline = Some(std::time::Instant::now() - std::time::Duration::from_secs(1));
    for (path, home, patterned, hidden) in [
        (path.as_str(), "/h", true, false),
        ("/h/.ssh/CONFIG", "/h", false, true),
    ] {
        assert_eq!(
            owner.check(path, home, patterned, hidden).unwrap_err().kind,
            crate::CheckErrorKind::Deadline
        );
    }
    assert_eq!(
        owner.broad("/h", "/h", false).unwrap_err().kind,
        crate::CheckErrorKind::Deadline
    );
}

#[test]
fn link_walk_reuses_domains_without_reusing_probes() {
    use super::super::{FirmlinkTable, Identity, Metadata, Probe, Resolver};
    use crate::record::{Effect, Target, Via, Walk};
    #[derive(Default)]
    struct Counted {
        readlinks: usize,
    }
    impl Probe for Counted {
        fn read_link(
            &mut self,
            _: &std::path::Path,
        ) -> std::io::Result<Option<std::path::PathBuf>> {
            self.readlinks += 1;
            Ok(None)
        }
        fn stat(&mut self, _: &std::path::Path) -> std::io::Result<Option<Metadata>> {
            panic!("unrelated public target must not stat");
        }
    }
    for width in [2, 4, 8, 16] {
        for depth in [1, 2, 4, 8] {
            let table = FirmlinkTable::from_text("");
            let mut resolver = Resolver::new("/h", &table);
            let mut probe = Counted::default();
            for index in 0..width {
                for effect in [Effect::Read, Effect::Use] {
                    let path = format!("/p/{}/file{index}", vec!["public"; depth].join("/"));
                    let mut target = Target::new(path, effect, Walk::None, Via::Operand);
                    assert!(matches!(
                        resolver.target(&mut target, "/p", &mut probe).unwrap(),
                        Identity::Public(_)
                    ));
                }
            }
            assert!(resolver.lexical.candidate_evaluations <= width + depth + 8);
            assert_eq!(resolver.lexical.domain_preparations, 1);
            assert!(probe.readlinks >= width * 2 * (depth + 2));
            println!(
                "walk width={width}, depth={depth}, candidates={}, readlinks={}",
                resolver.lexical.candidate_evaluations, probe.readlinks
            );
        }
    }
}

#[test]
fn cached_public_prefix_does_not_hide_a_later_probe_fault() {
    use super::super::{FirmlinkTable, Identity, Metadata, Probe, Resolver};
    use crate::record::{Effect, Target, Via, Walk};
    struct Changing {
        fault: bool,
        calls: usize,
    }
    impl Probe for Changing {
        fn read_link(
            &mut self,
            path: &std::path::Path,
        ) -> std::io::Result<Option<std::path::PathBuf>> {
            self.calls += 1;
            if self.fault && path == std::path::Path::new("/p") {
                return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
            }
            Ok(None)
        }
        fn stat(&mut self, _: &std::path::Path) -> std::io::Result<Option<Metadata>> {
            panic!("unexpected stat");
        }
    }
    let table = FirmlinkTable::from_text("");
    let mut owner = Resolver::new("/h", &table);
    let mut probe = Changing {
        fault: false,
        calls: 0,
    };
    let mut first = Target::new("/p/first".into(), Effect::Use, Walk::None, Via::Operand);
    assert!(matches!(
        owner.target(&mut first, "/p", &mut probe).unwrap(),
        Identity::Public(_)
    ));
    let before = probe.calls;
    probe.fault = true;
    let mut second = Target::new("/p/second".into(), Effect::Use, Walk::None, Via::Operand);
    assert_eq!(
        owner
            .target(&mut second, "/p", &mut probe)
            .unwrap_err()
            .kind,
        crate::CheckErrorKind::ProbeFault
    );
    assert_eq!(probe.calls, before + 1);
}
