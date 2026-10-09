use super::*;
use crate::{CoverageGap, record::HostFacts, shell::Arm};
fn fixture() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust-m2-scope-bounds.json"
    ))
    .unwrap()
}

fn seeded_scope() -> Scope {
    let values = fixture()["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| BindingValue::Known(v.as_str().unwrap().into()))
        .collect::<Vec<_>>();
    let mut scope = Scope::new("/h", "/p");
    for name in ["A", "B", "C"] {
        scope.assign(name.into(), values.clone());
    }
    scope
}

fn observation(source: &str, scope: &mut Scope) -> Observation {
    let mut output = Observation::default();
    let frontend = Frontend {
        arm: Arm::Brush,
        zsh: true,
        host: HostFacts {
            home: "/h",
            user: None,
        },
    };
    let mut evaluator = Evaluator::new(frontend, &mut output);
    evaluator.source(source, scope, 0).unwrap();
    evaluator.finish();
    output
}

mod bounds;
mod candidate_cost;
mod directory;
mod expansion;
mod flow;
mod loop_cost;
mod snapshot_cost;
