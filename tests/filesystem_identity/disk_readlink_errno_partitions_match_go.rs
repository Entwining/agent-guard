use crate::support;
use agent_guard_rust::filesystem::{DiskProbe, Probe};

#[test]
fn disk_readlink_errno_partitions_match_go() {
    let mut probe = DiskProbe;
    let fixture = support::Fixture::new();
    let root = fixture.root.join("fs-errno");
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("ordinary");
    std::fs::write(&file, "public").unwrap();
    for path in [
        file.clone(),
        root.join("absent"),
        file.join("child"),
        root.join("x".repeat(1024)),
    ] {
        assert_eq!(probe.read_link(&path).unwrap(), None, "{}", path.display());
    }
    let a = root.join("a");
    let b = root.join("b");
    std::os::unix::fs::symlink(&b, &a).unwrap();
    std::os::unix::fs::symlink(&a, &b).unwrap();
    for path in [a.clone(), a.join("child")] {
        assert!(probe.stat(&path).is_err(), "{}", path.display());
    }
    // A direct readlink returns the link bytes; traversing a cyclic parent fails.
    assert!(probe.read_link(&a.join("child")).is_err());
}
