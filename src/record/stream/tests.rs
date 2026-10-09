use super::*;

#[test]
fn branch_tags_do_not_make_unknown_output_known() {
    let mut builder = Builder::default();
    let tag = builder.bytes(String::new(), Guard::from([(3, 1)]));
    let unknown = builder.unknown();
    let flow = builder.sequence(vec![tag, unknown]);
    let values = builder.candidates(&flow).unwrap();
    assert!(values.iter().all(|value| value.unknown && !value.known));
    assert_eq!(values[0].guard, Guard::from([(3, 1)]));
}

#[test]
fn repeated_captured_input_stops_at_the_input_byte_boundary() {
    let counts = [8, 16, 32].map(|depth| {
        let mut builder = Builder::default();
        let mut flow = builder.bytes("x".into(), Guard::new());
        for _ in 0..depth {
            flow = builder.sequence(vec![flow.clone(), flow]);
        }
        let values = builder.candidates(&flow).unwrap();
        if depth < 18 {
            assert_eq!(values[0].text.len(), 1 << depth);
            assert!(builder.gap().is_none());
        } else {
            assert!(values.is_empty());
            assert_eq!(builder.gap(), Some(CoverageGap::InspectionBudget));
        }
        assert!(builder.nodes <= depth + 1);
        assert!(builder.visits <= depth * 2 + 1);
        assert!(builder.pairs <= depth * 2);
        (builder.nodes, builder.visits, builder.pairs)
    });
    for pair in counts.windows(2) {
        assert!(
            pair[1].0 <= pair[0].0 * 3 && pair[1].1 <= pair[0].1 * 3 && pair[1].2 <= pair[0].2 * 3,
            "{counts:?}"
        );
    }
}
