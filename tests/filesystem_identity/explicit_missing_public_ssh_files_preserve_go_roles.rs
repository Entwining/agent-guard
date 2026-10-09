use crate::program_contract as contract;
use agent_guard_rust::filesystem::{self, Identity, Metadata, Probe};
use std::{
    io,
    path::{Path, PathBuf},
};

#[test]
fn explicit_missing_public_ssh_files_preserve_go_roles() {
    contract::partition("control", include_str!("../fixtures/rust-batch10-ssh.json"));
}

#[test]
fn missing_ssh_root_has_no_inode_identity() {
    let mut probe = Absent;
    let home = "/synthetic/missing/home";
    let path = format!("{home}/.ssh/config");
    let result = filesystem::identify_scope(&path, home, home, true, &mut probe).unwrap();
    assert!(matches!(result, Identity::Public(_)), "{result:?}");
}

struct Absent;
impl Probe for Absent {
    fn read_link(&mut self, _: &Path) -> io::Result<Option<PathBuf>> {
        Ok(None)
    }
    fn stat(&mut self, _: &Path) -> io::Result<Option<Metadata>> {
        Ok(None)
    }
}
