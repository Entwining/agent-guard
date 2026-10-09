use super::*;
#[test]
fn literal_walk_checks_deadline_after_each_probe() {
    struct Expiring {
        deadline: std::time::Instant,
        calls: usize,
    }
    impl Probe for Expiring {
        fn read_link(&mut self, _: &Path) -> std::io::Result<Option<std::path::PathBuf>> {
            self.calls += 1;
            if self.calls == 1 {
                std::thread::sleep(
                    self.deadline
                        .saturating_duration_since(std::time::Instant::now()),
                );
                Ok(None)
            } else {
                Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
            }
        }
        fn stat(&mut self, _: &Path) -> std::io::Result<Option<super::super::Metadata>> {
            panic!("link walk must not stat");
        }
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(25);
    let mut probe = Expiring { deadline, calls: 0 };
    let mut lexical = Lexical::new(Some(deadline));
    let result = follow(
        "/public/next",
        "/synthetic/home",
        None,
        &FirmlinkTable::from_text(""),
        &mut probe,
        &mut lexical,
    );
    assert_eq!(result.err().unwrap().kind, CheckErrorKind::Deadline);
    assert!(probe.calls <= 1);
    println!("probe_calls_before_deadline={}", probe.calls);
}
#[test]
fn literal_walk_work_grows_with_path_bytes() {
    struct NoLinks;
    impl Probe for NoLinks {
        fn read_link(&mut self, _: &Path) -> std::io::Result<Option<std::path::PathBuf>> {
            Ok(None)
        }
        fn stat(&mut self, _: &Path) -> std::io::Result<Option<super::super::Metadata>> {
            panic!("public literal identity does not need stat");
        }
    }
    for (depth, (expands, glob, basename)) in
        [128, 256, 512, 1024, 2048].into_iter().flat_map(|depth| {
            [
                (false, false, "file.txt"),
                (true, false, "file.txt"),
                (true, true, "*.txt"),
            ]
            .into_iter()
            .map(move |form| (depth, form))
        })
    {
        let path = format!("/public/{}{basename}", "p/".repeat(depth));
        let table = FirmlinkTable::from_text("");
        let mut owner = super::super::Resolver::new("/synthetic/home", &table);
        let mut target = crate::record::Target::new(
            path.clone(),
            crate::record::Effect::Read,
            crate::record::Walk::None,
            crate::record::Via::Operand,
        );
        target.expands = expands;
        target.glob = glob;
        assert_eq!(
            owner.target(&mut target, "/public", &mut NoLinks).unwrap(),
            super::super::Identity::Public(path.clone())
        );
        println!(
            "depth={depth}, expands={expands}, glob={glob}, path_bytes={}, lexical_bytes={}",
            path.len(),
            owner.lexical.classified_bytes
        );
        assert!(
            owner.lexical.classified_bytes <= 8 * path.len(),
            "prefix walk rescanned path bytes at depth {depth}"
        );
    }
}
#[test]
fn catalog_read_fault_is_probe_fault() {
    let result = FirmlinkTable::from_read(Err(std::io::Error::from(
        std::io::ErrorKind::PermissionDenied,
    )));
    assert!(matches!(
        result,
        Err(CheckError {
            kind: CheckErrorKind::ProbeFault
        })
    ));
}
