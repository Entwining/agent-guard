mod patterns;

use super::*;
use crate::record::{Effect, HostFacts, Target, Via, Walk, Word};
use std::collections::BTreeMap;

#[derive(Default)]
struct Mock {
    links: BTreeMap<String, String>,
    calls: Vec<String>,
}
impl Probe for Mock {
    fn read_link(&mut self, path: &Path) -> io::Result<Option<PathBuf>> {
        let path = path.to_str().unwrap();
        assert!(
            lexical_literal(path, "/h").is_none(),
            "protected probe: {path}"
        );
        self.calls.push(path.into());
        Ok(self.links.get(path).map(PathBuf::from))
    }
    fn stat(&mut self, _: &Path) -> io::Result<Option<Metadata>> {
        panic!("unexpected stat");
    }
}

#[test]
fn broad_patterns_compare_literal_roots_and_active_wildcards() {
    let home = "/synthetic/home-x+y@z*";
    let encoded = literal_shell_pattern(home);
    assert!(broad_root(&encoded, home, true));
    assert!(!broad_root(&format!("{encoded}*/*.md"), home, true));
    assert!(broad_root(&format!("{encoded}/**/*.md"), home, true));
}

#[test]
fn item_credentials_exclude_appdata_from_the_credential_partition() {
    let table = FirmlinkTable::from_text("");
    let mut resolver = Resolver::new("/h", &table);
    assert_eq!(
        resolver.credential("/p/.env", "/h", false).unwrap(),
        Some(Protection::Environment)
    );
    assert_eq!(
        resolver
            .credential("/h/Library/Containers/x", "/h", false)
            .unwrap(),
        None
    );
}

#[test]
fn credential_root_reads_remain_protected_before_public_alias_resolution() {
    let table = FirmlinkTable::from_text("");
    for (effect, via, expected) in [
        (Effect::Read, Via::Operand, true),
        (Effect::Use, Via::Operand, false),
        (Effect::Read, Via::Cwd, false),
    ] {
        let mut probe = Mock {
            links: BTreeMap::from([("/h/.docker".into(), "/public".into())]),
            calls: Vec::new(),
        };
        let mut resolver = Resolver::new("/h", &table);
        let mut target = Target::new("/h/.docker".into(), effect, Walk::None, via);
        assert_eq!(
            matches!(
                resolver.target(&mut target, "/p", &mut probe).unwrap(),
                Identity::Protected(Protection::Credential)
            ),
            expected
        );
        if expected {
            assert!(probe.calls.is_empty());
        }
    }
}

#[test]
fn aliased_credential_root_reads_keep_the_post_resolution_role() {
    let table = FirmlinkTable::from_text("");
    for (effect, via, expected) in [
        (Effect::Read, Via::Operand, true),
        (Effect::Use, Via::Operand, false),
        (Effect::Read, Via::Cwd, false),
    ] {
        let mut probe = Mock {
            links: BTreeMap::from([("/p/link".into(), "/h/.docker".into())]),
            calls: Vec::new(),
        };
        let mut resolver = Resolver::new("/h", &table);
        let mut target = Target::new("/p/link".into(), effect, Walk::None, via);
        assert_eq!(
            matches!(
                resolver.target(&mut target, "/p", &mut probe).unwrap(),
                Identity::Protected(Protection::Credential)
            ),
            expected
        );
        assert!(probe.calls.contains(&"/p/link".to_owned()));
    }
}

#[test]
fn lexical_api_keeps_relative_and_short_pattern_boundaries() {
    assert_eq!(
        lexical_pattern("/h/Library/Containers/x", "/h", false),
        Some(Protection::AppData)
    );
    assert_eq!(lexical_pattern("/b*", "/h", true), None);
    assert_eq!(lexical_pattern("/b*.txt", "/h", true), None);
    assert_eq!(
        lexical_pattern(".aws/c*", "/h", true),
        Some(Protection::Credential)
    );
    assert_eq!(
        lexical_pattern("/.config/gh/host*", "/h", true),
        Some(Protection::Credential)
    );
}

#[test]
fn catalog_controls_physical_parent_transition() {
    let packet: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/rust-m2-filesystem.json")).unwrap();
    let rows: Vec<_> = packet["paths"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r.get("catalog").is_some() && r["operation"] != "shell")
        .collect();
    assert!(!rows.is_empty(), "missing catalog non-shell partition");
    for row in rows {
        let table = FirmlinkTable::from_text(row["catalog"].as_str().unwrap());
        let mut resolver = Resolver::new("/h", &table);
        let mut probe = Mock {
            links: row
                .get("links")
                .map(|v| serde_json::from_value(v.clone()).unwrap())
                .unwrap_or_default(),
            calls: Vec::new(),
        };
        let mut target = Target::new(
            row["path"].as_str().unwrap().into(),
            Effect::Use,
            Walk::None,
            Via::Operand,
        );
        let result = resolver
            .target(
                &mut target,
                row["cwd"].as_str().unwrap_or("/project"),
                &mut probe,
            )
            .unwrap();
        if let Some(kind) = row["protected"].as_str() {
            assert!(
                matches!(result, Identity::Protected(p) if format!("{p:?}") == kind),
                "{}: {result:?}",
                row["id"]
            );
        } else {
            assert_eq!(
                result,
                Identity::Public(row["public"].as_str().unwrap().into()),
                "{}",
                row["id"]
            );
        }
    }
}

#[test]
fn target_preserves_raw_input_and_updates_both_linked_fields() {
    let mut word = Word::literal("../item".into());
    word.expands = true;
    let mut target = Target::from_word(
        &word,
        "/System/Volumes/Data/a/link/./tail",
        HostFacts {
            home: "/h",
            user: None,
        },
        Effect::Use,
        Walk::Visible,
    );
    assert_eq!(target.path, "/a/link/item");
    assert_eq!(
        target.unresolved,
        "/System/Volumes/Data/a/link/./tail/../item"
    );
    target.command = Some(3);
    target.sends = true;
    target.search = true;
    let table = FirmlinkTable::from_text("");
    let mut resolver = Resolver::new("/h", &table);
    let mut probe = Mock {
        links: BTreeMap::from([("/System/Volumes/Data/a/link".into(), "/public".into())]),
        calls: Vec::new(),
    };
    assert_eq!(
        resolver
            .target(&mut target, "/project", &mut probe)
            .unwrap(),
        Identity::Public("/public/item".into())
    );
    assert_eq!(target.path, "/public/item");
    assert_eq!(target.unresolved, target.path);
    assert!(target.expands && target.sends && target.search);
    assert_eq!(
        (target.effect, target.walk, target.via, target.command),
        (Effect::Use, Walk::Visible, Via::Operand, Some(3))
    );
}

#[test]
fn literal_name_and_tool_glob_skip_identity() {
    let table = FirmlinkTable::from_text("");
    let mut resolver = Resolver::new("/h", &table);
    let mut probe = Mock::default();
    for (path, effect, via, glob) in [
        ("/public/link/*", Effect::Name, Via::Operand, false),
        ("/public/link/*.txt", Effect::Read, Via::Tool, true),
    ] {
        let mut target = Target::new(path.into(), effect, Walk::None, via);
        target.glob = glob;
        assert_eq!(
            resolver
                .target(&mut target, "/project", &mut probe)
                .unwrap(),
            Identity::Public(path.into())
        );
    }
    assert!(probe.calls.is_empty());
}

#[test]
fn evaluation_keeps_no_follow_input() {
    let table = FirmlinkTable::from_text("");
    let mut resolver = Resolver::new("/h", &table);
    let mut probe = Mock::default();
    for path in ["/a/./item", "/b/../item"] {
        let mut target = Target::new(path.into(), Effect::Use, Walk::None, Via::Operand);
        let unresolved = target.unresolved.clone();
        let clean = target.path.clone();
        let identity = resolver
            .target(&mut target, "/project", &mut probe)
            .unwrap();
        assert_eq!(identity, Identity::Public(clean.clone()));
        assert_eq!(target.path, clean);
        assert_eq!(target.unresolved, unresolved);
    }
    assert!(probe.calls.iter().any(|p| p == "/h"));
    assert!(probe.calls.iter().all(|p| !p.contains("/./")));
    let mut target = Target::new("//*.txt".into(), Effect::Use, Walk::None, Via::Operand);
    target.glob = true;
    assert_eq!(
        resolver
            .target(&mut target, "/project", &mut probe)
            .unwrap(),
        Identity::Public("/*.txt".into())
    );
    // policy::Inspection::target reads unresolved to distinguish relative tilde spellings.
    assert_eq!(target.unresolved, "//*.txt");
}

#[test]
fn expands_without_glob_follows_only_public_prefix() {
    let packet: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/rust-m2-filesystem.json")).unwrap();
    let row = packet["paths"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "expands-only-suffix")
        .unwrap();
    let mut word = Word::literal(row["path"].as_str().unwrap().into());
    word.expands = true;
    let mut target = Target::from_word(
        &word,
        "/project",
        HostFacts {
            home: "/h",
            user: None,
        },
        Effect::Use,
        Walk::None,
    );
    let table = FirmlinkTable::from_text("");
    let mut resolver = Resolver::new("/h", &table);
    let mut probe = Mock {
        links: serde_json::from_value(row["links"].clone()).unwrap(),
        calls: Vec::new(),
    };
    assert_eq!(
        resolver
            .target(&mut target, "/project", &mut probe)
            .unwrap(),
        Identity::Public(row["public"].as_str().unwrap().into())
    );
    assert_eq!(
        probe.calls.last().unwrap(),
        row["last_probe"].as_str().unwrap()
    );
    assert!(probe.calls.iter().all(|path| !path.contains("${suffix}")));
}
