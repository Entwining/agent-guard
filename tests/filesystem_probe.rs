mod support;
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
}
