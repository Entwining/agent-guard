use super::*;

#[test]
fn pipeline_stage_work_stops_at_the_command_budget() {
    let visits = [1024, 4096].map(|stages| {
        let output = observation(&vec!["true"; stages].join("|"), &mut Scope::new("/h", "/p"));
        assert!(output.gaps.contains(&CoverageGap::InspectionBudget));
        output.cwd_candidates
    });
    println!("pipeline stage visits: {visits:?}");
    assert_eq!(visits[0], visits[1]);
}
