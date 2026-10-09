#[test]
fn public_ssh_backup_suffixes_keep_lexical_identity() {
    crate::program_contract::partition(
        "backup",
        include_str!("../fixtures/rust-public-ssh-backups.json"),
    );
}
