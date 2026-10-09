use super::*;
#[test]
fn runtime_glob_propagation_stays_unknown() {
    let data: serde_json::Value =
        serde_json::from_str(include_str!("../../../../tests/fixtures/rust-m2-1.json")).unwrap();
    let row = data["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "loop-unknown-glob")
        .unwrap();
    let mut scope = Scope::new("/h", "/p");
    let result = observation(row["source"].as_str().unwrap(), &mut scope);
    assert!(
        scope.bindings["D"].values.iter().any(|value| value
            .lexical()
            .is_some_and(|text| text == "public*")
            && value.known().is_none()),
        "{:?}: {result:?}",
        scope.bindings
    );
    assert!(
        result
            .script
            .commands
            .iter()
            .flat_map(|command| &command.argv)
            .any(|word| word.text == "public*" && word.shell_matches)
    );
}

#[test]
fn finite_loop_retains_all_directory_candidates() {
    let data: serde_json::Value =
        serde_json::from_str(include_str!("../../../../tests/fixtures/rust-m2-1.json")).unwrap();
    let row = data["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "loop-cwd-up-three")
        .unwrap();
    let mut scope = Scope::new("/h", row["cwd"].as_str().unwrap());
    let result = observation(row["source"].as_str().unwrap(), &mut scope);
    assert!(
        scope
            .directory
            .alternatives
            .iter()
            .any(|path| path.render() == "/h/project")
    );
    assert_eq!(scope.directory.current.render(), "/h");
    assert!(
        !result.gaps.contains(&CoverageGap::InspectionBudget),
        "{result:?}"
    );
}

#[test]
fn tracked_movement_updates_oldpwd_binding() {
    let data: serde_json::Value =
        serde_json::from_str(include_str!("../../../../tests/fixtures/rust-m2-1.json")).unwrap();
    let row = data["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "oldpwd-metadata-control")
        .unwrap();
    let mut scope = Scope::new("/h", "/h/project");
    observation(row["source"].as_str().unwrap(), &mut scope);
    assert!(
        scope.bindings["OLDPWD"]
            .values
            .contains(&BindingValue::Known("/h/public".into()))
    );
    assert_eq!(scope.directory.current.render(), "/h/project");
}

#[test]
fn relative_cd_cannot_refine_a_carried_glob_cwd() {
    let mut scope = Scope::new("/h", "/h/project");
    let result = observation("for d in public*/; do cd \"$d\"; done; cd ..", &mut scope);
    assert!(result.gaps.contains(&CoverageGap::IdentityBound));
    assert_eq!(scope.directory.current.render(), "/h/project");
    assert!(scope.directory.alternatives.is_empty());
}

#[test]
fn directory_alternatives_keep_order_and_collapse() {
    let paths = (0..17)
        .map(|n| cwd::CwdPath::Logical(format!("/p/{n}")))
        .collect::<Vec<_>>();
    let current = cwd::CwdPath::Logical("/p".into());
    assert_eq!(
        cwd::bounded(&current, paths[..16].to_vec(), "/h"),
        (paths[..16].to_vec(), None)
    );
    assert_eq!(
        cwd::bounded(&current, paths, "/h"),
        (
            vec![
                cwd::CwdPath::Logical("/h".into()),
                cwd::CwdPath::Logical("/h/Library".into())
            ],
            Some(crate::CoverageGap::InspectionBudget)
        )
    );
}
