mod support;
use agent_guard_rust::{
    CheckErrorKind, Event, Outcome,
    adapters::render,
    evaluate_with_arm,
    filesystem::{self, FileKind, Identity, Metadata, Probe, Protection},
};
use serde_json::json;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[test]
fn ssh_public_file_type_and_probe_fault() {
    let fixture = support::Fixture::new();
    for name in ["config.d", "directory.pub", "known_hosts.backup"] {
        fs::create_dir(format!("{}/.ssh/{name}", fixture.home)).unwrap();
    }
    fs::write(
        format!("{}/.ssh/config", fixture.home),
        "public configuration",
    )
    .unwrap();
    fs::hard_link(
        format!("{}/.ssh/config", fixture.home),
        format!("{}/.ssh/config.work", fixture.home),
    )
    .unwrap();
    for &arm in agent_guard_rust::shell::ACCEPTANCE_ARMS {
        let context = fixture.context(&json!({"consumer":"codex", "cwd":"$P"}));
        for (name, expected) in [
            ("config.d", "D"),
            ("directory.pub", "D"),
            ("known_hosts.backup", "D"),
            ("config", "N"),
            ("config.work", "N"),
        ] {
            let body = serde_json::to_vec(&json!({"tool_name":"Bash", "tool_input":{"command":format!("cat {}/.ssh/{name}", fixture.home)}})).unwrap();
            let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
            let result = evaluate_with_arm(
                Event {
                    bytes: &body,
                    context: &context,
                    probe: &mut probe,
                },
                arm,
            );
            assert_eq!(
                support::class(&result),
                expected,
                "{name} {arm:?}: {result:?}"
            );
            assert!(
                probe
                    .stat_calls
                    .contains(&format!("{}/.ssh/{name}", fixture.home))
            );
        }
        let body = serde_json::to_vec(&json!({"tool_name":"Bash", "tool_input":{"command":format!("cat {}/.ssh/config", fixture.home)}})).unwrap();
        let mut probe = StatFault {
            inner: support::RecordingProbe::literal_for_quoted_paths(&fixture),
        };
        let result = evaluate_with_arm(
            Event {
                bytes: &body,
                context: &context,
                probe: &mut probe,
            },
            arm,
        );
        assert_eq!(render(context.consumer, &result).exit, 2);
        assert_eq!(result.unwrap_err().kind, CheckErrorKind::ProbeFault);
    }
}

struct StatFault {
    inner: support::RecordingProbe,
}

#[test]
fn protected_ssh_root_does_not_deny_unrelated_public_targets() {
    let fixture = support::Fixture::new();
    fs::rename(
        format!("{}/.ssh", fixture.home),
        fixture.root.join("old-ssh"),
    )
    .unwrap();
    std::os::unix::fs::symlink(&fixture.container, format!("{}/.ssh", fixture.home)).unwrap();
    let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
    let identity = filesystem::identify(
        &format!("{}/input.txt", fixture.project),
        &fixture.project,
        &fixture.home,
        &mut probe,
    )
    .unwrap();
    assert!(matches!(identity, Identity::Public(_)));
    assert!(probe.stat_calls.is_empty());
    assert!(
        !probe
            .calls
            .iter()
            .any(|path| path.starts_with(&fixture.container))
    );
}
impl Probe for StatFault {
    fn read_link(&mut self, path: &Path) -> io::Result<Option<PathBuf>> {
        self.inner.read_link(path)
    }
    fn stat(&mut self, _: &Path) -> io::Result<Option<Metadata>> {
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }
}

#[test]
fn resolved_ssh_root_and_parent_inode_identity() {
    let fixture = support::Fixture::new();
    let root = fixture.root.join("ssh-store");
    fs::rename(format!("{}/.ssh", fixture.home), &root).unwrap();
    std::os::unix::fs::symlink(&root, format!("{}/.ssh", fixture.home)).unwrap();
    fs::create_dir(root.join("sub")).unwrap();
    fs::write(root.join("sub/config.pub"), "synthetic public material").unwrap();
    let mut ancestry = support::RecordingProbe::literal_for_quoted_paths(&fixture);
    assert_eq!(
        filesystem::identify(
            root.join("sub/config.pub").to_str().unwrap(),
            &fixture.project,
            &fixture.home,
            &mut ancestry
        )
        .unwrap(),
        Identity::Protected(Protection::SshPrivate)
    );
    for &arm in agent_guard_rust::shell::ACCEPTANCE_ARMS {
        let context = fixture.context(&json!({"consumer":"codex", "cwd":"$P"}));
        let body = serde_json::to_vec(&json!({"tool_name":"Bash", "tool_input":{"command":format!("cat {}", root.display())}})).unwrap();
        let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
        let result = evaluate_with_arm(
            Event {
                bytes: &body,
                context: &context,
                probe: &mut probe,
            },
            arm,
        );
        assert!(matches!(
            result.as_ref().unwrap().outcome,
            Outcome::ProtectedDenial { .. }
        ));
        assert_eq!(render(context.consumer, &result).exit, 2);
    }
    // A metadata identity alias models the same-file comparison without relying
    // on directory hard links, which the host does not permit creating.
    let alias = root.join("alias/config.pub");
    let mut probe = ParentAlias {
        root: root.clone(),
        alias_parent: alias.parent().unwrap().to_owned(),
        inner: support::RecordingProbe::literal_for_quoted_paths(&fixture),
    };
    let identity = filesystem::identify(
        alias.to_str().unwrap(),
        &fixture.project,
        &fixture.home,
        &mut probe,
    )
    .unwrap();
    assert_eq!(identity, Identity::Protected(Protection::SshPrivate));
    assert!(
        probe
            .inner
            .stat_calls
            .contains(&alias.parent().unwrap().to_str().unwrap().to_owned())
    );
}

struct ParentAlias {
    root: PathBuf,
    alias_parent: PathBuf,
    inner: support::RecordingProbe,
}

#[test]
fn search_compares_resolved_ssh_root_parents() {
    let fixture = support::Fixture::new();
    let parent = fixture.root.join("storage");
    fs::create_dir(&parent).unwrap();
    fs::rename(format!("{}/.ssh", fixture.home), parent.join("keys")).unwrap();
    std::os::unix::fs::symlink(parent.join("keys"), format!("{}/.ssh", fixture.home)).unwrap();
    for search in [false, true] {
        let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
        let identity = filesystem::identify_scope(
            parent.to_str().unwrap(),
            &fixture.project,
            &fixture.home,
            search,
            &mut probe,
        )
        .unwrap();
        assert_eq!(
            matches!(identity, Identity::Protected(Protection::SshPrivate)),
            search
        );
        if search {
            assert!(
                probe
                    .stat_calls
                    .contains(&parent.to_str().unwrap().to_owned())
            );
        } else {
            assert!(probe.stat_calls.is_empty());
        }
    }
}
impl Probe for ParentAlias {
    fn read_link(&mut self, path: &Path) -> io::Result<Option<PathBuf>> {
        self.inner.read_link(path)
    }
    fn stat(&mut self, path: &Path) -> io::Result<Option<Metadata>> {
        self.inner
            .stat_calls
            .push(path.to_str().unwrap().to_owned());
        let inode = if path == self.root || path == self.alias_parent || path.ends_with(".ssh") {
            1
        } else {
            2
        };
        Ok(Some(Metadata {
            device: 1,
            inode,
            kind: FileKind::Directory,
        }))
    }
}

#[test]
fn private_key_spelling_stops_before_all_probes() {
    let fixture = support::Fixture::new();
    for name in [
        "id_rsa",
        "id_ed25519",
        "keys/id.pub",
        "CONFIG",
        "id.PUB",
        "known_HOSTS",
    ] {
        let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
        let identity = filesystem::identify(
            &format!("{}/.ssh/{name}", fixture.home),
            &fixture.project,
            &fixture.home,
            &mut probe,
        )
        .unwrap();
        assert_eq!(identity, Identity::Protected(Protection::SshPrivate));
        assert!(probe.calls.is_empty());
        assert!(probe.stat_calls.is_empty());
        println!(
            "{}",
            json!({"spelling":format!("$H/.ssh/{name}"), "readlink":probe.calls, "stat":probe.stat_calls})
        );
    }
}
