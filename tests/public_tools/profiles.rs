use super::*;

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "This integration test checks the generated synthetic profile's private permission mode."
)]
fn profile_keeps_selected_identity_denial_uuids_permissions_and_existing_bytes() {
    for (kind, identifier) in [
        ("bundleID", "com.example.canary"),
        ("path", "/Applications/Canary & \"Test\"/client"),
    ] {
        let root = root();
        let output = root.path().join("appdata.mobileconfig");
        let requirement = "identifier \"com.example.canary\" and anchor apple";
        let args = [kind, identifier, requirement, output.to_str().unwrap()].map(String::from);
        let mut printed = Vec::new();
        profile::generate(&args, &mut printed).unwrap();
        let converted = Command::new("/usr/bin/plutil")
            .args(["-convert", "json", "-o", "-"])
            .arg(&output)
            .output()
            .unwrap();
        assert!(converted.status.success());
        let value: serde_json::Value = serde_json::from_slice(&converted.stdout).unwrap();
        assert_eq!(value["PayloadType"], "Configuration");
        assert_eq!(value["PayloadScope"], "System");
        let content = value["PayloadContent"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        let payload = &content[0];
        assert_eq!(
            payload["PayloadType"],
            "com.apple.TCC.configuration-profile-policy"
        );
        assert_eq!(payload["Services"].as_object().unwrap().len(), 1);
        let rows = payload["Services"]["SystemPolicyAppData"]
            .as_array()
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["Identifier"], identifier);
        assert_eq!(rows[0]["IdentifierType"], kind);
        assert_eq!(rows[0]["CodeRequirement"], requirement);
        assert_eq!(rows[0]["Allowed"], false);
        let uuid = |text: &str| {
            let parts = text.split('-').collect::<Vec<_>>();
            assert_eq!(
                parts.iter().map(|part| part.len()).collect::<Vec<_>>(),
                [8, 4, 4, 4, 12]
            );
            assert!(
                parts
                    .iter()
                    .all(|part| part.bytes().all(|byte| byte.is_ascii_hexdigit()))
            );
            assert!(parts[2].starts_with('4'));
            assert!(matches!(parts[3].as_bytes()[0], b'8' | b'9' | b'A' | b'B'));
        };
        uuid(value["PayloadUUID"].as_str().unwrap());
        uuid(payload["PayloadUUID"].as_str().unwrap());
        assert_ne!(value["PayloadUUID"], payload["PayloadUUID"]);
        assert_eq!(
            payload["PayloadIdentifier"],
            format!("{}.pppc", value["PayloadIdentifier"].as_str().unwrap())
        );
        assert_eq!(
            fs::metadata(&output).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let printed = String::from_utf8(printed).unwrap();
        for text in [
            "Plist validation: PASS",
            "Runtime enforcement: unverified",
            "A supervised Mac running macOS 14 or newer, admin rights",
            "baseline must open successfully",
            "Manual profile installation",
            "Allowed=false",
        ] {
            assert!(printed.contains(text));
        }
        let before = fs::read(&output).unwrap();
        assert!(profile::generate(&args, &mut Vec::new()).is_err());
        assert_eq!(fs::read(&output).unwrap(), before);
    }
}

#[test]
fn invalid_profiles_write_nothing() {
    let root = root();
    let output = root.path().join("invalid.mobileconfig");
    let output = output.to_str().unwrap();
    for args in [
        ["unknown", "com.example.canary", "anchor apple", output],
        ["path", "relative", "anchor apple", output],
        [
            "bundleID",
            "com.example.canary",
            "not a requirement",
            output,
        ],
        [
            "bundleID",
            "com.example.canary",
            "designated => identifier \"com.example.canary\"",
            output,
        ],
        ["bundleID", "com.example.canary", "anchor apple\n", output],
        ["bundleID", "com.example.canary\n", "anchor apple", output],
        ["bundleID", "invalid&identifier", "anchor apple", output],
        [
            "bundleID",
            "com.example.canary",
            "anchor apple",
            "relative.mobileconfig",
        ],
    ] {
        assert!(
            profile::generate(&args.map(String::from), &mut Vec::new()).is_err(),
            "{args:?}"
        );
        assert!(!Path::new(args[3]).exists());
    }
}

#[test]
fn profile_rejects_checkout_aliases_and_preserves_existing_symlink_target() {
    let root = root();
    let checkout = root.path().join("checkout");
    fs::create_dir(&checkout).unwrap();
    fs::write(checkout.join(".git"), "").unwrap();
    let alias = root.path().join("alias");
    symlink(&checkout, &alias).unwrap();
    for parent in [checkout, alias] {
        let output = parent.join("profile.mobileconfig");
        let args = [
            "bundleID",
            "com.example.canary",
            "anchor apple",
            output.to_str().unwrap(),
        ]
        .map(String::from);
        assert!(
            profile::generate(&args, &mut Vec::new())
                .unwrap_err()
                .contains("outside every checkout")
        );
    }
    let target = root.path().join("preserved");
    fs::write(&target, "preserved").unwrap();
    let link = root.path().join("profile.mobileconfig");
    symlink(&target, &link).unwrap();
    let args = [
        "bundleID",
        "com.example.canary",
        "anchor apple",
        link.to_str().unwrap(),
    ]
    .map(String::from);
    assert!(profile::generate(&args, &mut Vec::new()).is_err());
    assert_eq!(fs::read_to_string(target).unwrap(), "preserved");
}

#[test]
fn instructions_need_no_selected_client_and_cli_reports_usage() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-guard-profile"))
        .arg("--instructions")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).unwrap();
    for part in [
        "/usr/bin/profiles status -type enrollment",
        "baseline must open successfully",
        "cargo run --bin agent-guard-profile -- 'bundleID-or-path'",
        "enforcement remain unverified",
    ] {
        assert!(text.contains(part));
    }
    let output = Command::new(env!("CARGO_BIN_EXE_agent-guard-profile"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("FAIL: expected identifier type"));
}

#[test]
fn written_profile_validation_failure_retains_the_artifact() {
    let root = root();
    let output = root.path().join("invalid.mobileconfig");
    assert!(
        profile::write_profile(&output, "not a plist")
            .unwrap_err()
            .contains("profile written but validation failed")
    );
    assert_eq!(fs::read_to_string(output).unwrap(), "not a plist");
}
