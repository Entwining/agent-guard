use super::*;

struct NoProbe {
    calls: usize,
}
impl Probe for NoProbe {
    fn read_link(&mut self, _: &std::path::Path) -> std::io::Result<Option<std::path::PathBuf>> {
        self.calls += 1;
        Ok(None)
    }
    fn stat(&mut self, _: &std::path::Path) -> std::io::Result<Option<filesystem::Metadata>> {
        self.calls += 1;
        Ok(None)
    }
}

#[test]
fn unchanged_resource_domains_share_broad_root_checks() {
    let context = Context {
        consumer: Consumer::Claude,
        home: "/h".into(),
        cwd: "/project".into(),
        user: None,
        zsh_executor: true,
        require_execution_owner: false,
        shell_observation_entries: std::cell::Cell::new(0),
    };
    let table = filesystem::FirmlinkTable::from_text("");
    for size in [8, 16, 32] {
        let mut probe = NoProbe { calls: 0 };
        let mut inspection = Inspection {
            context: &context,
            probe: &mut probe,
            resolver: filesystem::Resolver::new(&context.home, &table),
            arm: Arm::Brush,
            gaps: Vec::new(),
            denial: None,
            appdata_reason: None,
            advice: Vec::new(),
            executable_qualifier: false,
            effects: Vec::new(),
            source_entries: 0,
            broad_root_queries: 0,
            deadline: None,
        };
        inspection
            .shell(&"ssh host public*;".repeat(size), &context.cwd, 0)
            .unwrap();
        assert_eq!(inspection.broad_root_queries, size, "size={size}");
        assert!(inspection.denial.is_none() && inspection.gaps.is_empty());
    }
}

#[test]
fn inspection_recursion_frontier_is_independent_of_delimiter_depth() {
    let context = Context {
        consumer: Consumer::Claude,
        home: "/h".into(),
        cwd: "/p".into(),
        user: None,
        zsh_executor: true,
        require_execution_owner: false,
        shell_observation_entries: std::cell::Cell::new(0),
    };
    let table = filesystem::FirmlinkTable::from_text("");
    let mut probe = NoProbe { calls: 0 };
    let mut inspection = Inspection {
        context: &context,
        probe: &mut probe,
        resolver: filesystem::Resolver::new(&context.home, &table),
        arm: Arm::Brush,
        gaps: Vec::new(),
        denial: None,
        appdata_reason: None,
        advice: Vec::new(),
        executable_qualifier: false,
        effects: Vec::new(),
        source_entries: 0,
        broad_root_queries: 0,
        deadline: None,
    };
    inspection.shell("true", &context.cwd, 64).unwrap();
    assert_eq!(context.shell_observation_entries.get(), 1);
    assert_eq!(
        inspection.shell("true", &context.cwd, 65).unwrap_err().kind,
        CheckErrorKind::ResourceLimit
    );
    inspection.shell("sh -c true", &context.cwd, 64).unwrap();
    assert_eq!(context.shell_observation_entries.get(), 2);
}

#[test]
fn literal_eval_bodies_are_observed_once_in_their_scope() {
    let context = Context {
        consumer: Consumer::Claude,
        home: "/h".into(),
        user: None,
        cwd: "/project".into(),
        zsh_executor: true,
        require_execution_owner: false,
        shell_observation_entries: std::cell::Cell::new(0),
    };
    for n in [2, 4, 8, 16, 24] {
        let mut probe = NoProbe { calls: 0 };
        let catalog = filesystem::FirmlinkTable::from_text("");
        let mut inspection = Inspection {
            context: &context,
            probe: &mut probe,
            resolver: filesystem::Resolver::new(&context.home, &catalog),
            arm: Arm::Brush,
            gaps: Vec::new(),
            denial: None,
            appdata_reason: None,
            advice: Vec::new(),
            executable_qualifier: false,
            effects: Vec::new(),
            source_entries: 0,
            broad_root_queries: 0,
            deadline: None,
        };
        inspection
            .shell(&format!("{}cat .env", "eval ".repeat(n)), &context.cwd, 0)
            .unwrap();
        assert_eq!(
            inspection.denial.as_ref().unwrap().rule,
            crate::DenialRule::File
        );
        assert!(inspection.effects.contains(&EffectRecord::ProtectedTarget {
            protection: filesystem::Protection::Environment,
            write: false,
            source: EffectSource::Nested,
        }));
        assert!(inspection.gaps.is_empty());
        assert!(
            inspection.source_entries <= n + 1,
            "eval depth {n}: {} observations",
            inspection.source_entries
        );
    }
}

#[test]
fn catalog_initialization_fault_blocks_supported_request() {
    let context = Context {
        consumer: Consumer::Claude,
        home: "/h".into(),
        user: None,
        cwd: "/project".into(),
        zsh_executor: true,
        require_execution_owner: false,
        shell_observation_entries: std::cell::Cell::new(0),
    };
    let mut probe = NoProbe { calls: 0 };
    let loads = std::cell::Cell::new(0);
    let result = evaluate_with_catalog_loader(
        Event {
            bytes: br#"{"tool_name":"Read","tool_input":{"file_path":"public"}}"#,
            context: &context,
            probe: &mut probe,
        },
        Arm::Brush,
        || {
            loads.set(loads.get() + 1);
            Err(CheckError {
                kind: CheckErrorKind::ProbeFault,
            })
        },
        None,
        adapters::Protocol::Tool,
    );
    assert!(matches!(
        result,
        Err(CheckError {
            kind: CheckErrorKind::ProbeFault
        })
    ));
    let wire = adapters::render(context.consumer, &result);
    assert_eq!(wire.exit, 2);
    assert!(wire.stdout.is_empty() && wire.stderr.contains("could not complete this check"));
    assert_eq!(probe.calls, 0);
    assert_eq!(loads.get(), 1);
    assert_eq!(context.shell_observation_entries.get(), 0);
}
