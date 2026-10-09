#[path = "../support/cases/literal_for_lists_execute_each_value_exactly.rs"]
pub mod cases;
use cases::*;

#[test]
fn literal_for_lists_execute_each_value_exactly() {
    partition("literal-loop");
}

#[test]
fn unset_removes_only_executed_named_bindings() {
    partition("unset");
}
