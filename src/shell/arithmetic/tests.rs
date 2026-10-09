#[test]
fn constant_binding_arithmetic_evaluates_each_binding_once() {
    for width in [4, 8, 16] {
        let mut bindings = std::collections::BTreeMap::from([("n0".into(), "1".into())]);
        for n in 1..=width {
            bindings.insert(format!("n{n}"), format!("n{}+n{}", n - 1, n - 1));
        }
        let result = super::integer(
            &format!("n{width}-n{width}"),
            &bindings,
            &std::collections::BTreeSet::new(),
            None,
        )
        .unwrap();
        assert_eq!(result.value, Some(0));
        assert!(
            result.bindings_evaluated <= width + 1,
            "width={width}: {} binding evaluations",
            result.bindings_evaluated
        );
    }
}
