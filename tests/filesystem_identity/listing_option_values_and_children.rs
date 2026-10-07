#[path = "../support/cases/listing_option_values_and_children.rs"]
pub mod cases;
use cases::*;

#[test]
fn readlink_parents_and_firmlink_spelling() {
    required(&[
        "appdata[96]",
        "appdata[101]",
        "appdata[108]",
        "appdata[125]",
        "credentials[173]",
        "cwd[2]",
        "cwd[4]",
        "cwd[63]",
        "cwd[64]",
        "cwd[65]",
    ]);
}
