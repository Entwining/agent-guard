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
use super::*;

#[test]
fn armed_classifier_refuses_its_own_lexer_bound() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust-m2-arith-sinks.json"
    ))
    .unwrap();
    let row = fixture["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "armed-lexer-bound")
        .unwrap();
    let value = row["source"]
        .as_str()
        .unwrap()
        .strip_prefix("x='")
        .unwrap()
        .strip_suffix("'; echo x")
        .unwrap();
    assert!(matches!(armed(value), Arming::Unresolved));
}
