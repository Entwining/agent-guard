use super::*;

pub(super) fn assert_effect_or_failure(row: &Value, actual: &Value, conditional: bool) {
    let contract = if conditional {
        &row["conditional_outcome"]["reason_contract"]
    } else {
        &row["reason_contract"]
    };
    let Some(required) = contract["effect_or_failure"].as_str() else {
        assert_ne!(
            contract["emission"], "required",
            "required reason needs a semantic partition"
        );
        return;
    };
    let stderr = actual["stderr"].as_str().unwrap();
    let effects = actual["observed_effects"].as_array().unwrap();
    let protected = |kind: &str, write: bool| {
        effects
            .iter()
            .any(|effect| effect["protection"] == kind && effect["write"] == write)
    };
    let gaps = actual["coverage"]["gaps"].as_array();
    let gap = |name: &str| gaps.is_some_and(|values| values.iter().any(|value| value == name));
    let matches = if actual["class"] == "F" {
        actual["coverage"]["error_kind"] == required
    } else if required.contains("App Data") {
        protected("AppData", false)
            && stderr.contains("app-data")
            && (!required.contains("nested") && !required.contains("substitution")
                || effects.iter().any(|effect| {
                    effect["protection"] == "AppData" && effect["source"] == "Nested"
                }))
    } else if required.contains("SSH") || required.contains("private-key") {
        protected("SshPrivate", required.contains("write"))
            && (stderr.contains("credential or environment file")
                || stderr.contains("private material in the named .ssh directory"))
    } else if required.contains("broad") {
        effects.iter().any(|effect| effect["kind"] == "BroadRoot")
            && stderr.contains("home directory")
            && stderr.contains("Scope the scan to a project path")
    } else if required.contains("inline interpreter") {
        protected("Environment", false)
            && effects
                .iter()
                .any(|effect| effect["source"] == "InlineCode")
            && stderr.contains("inline code")
    } else if required.contains("hidden") {
        effects
            .iter()
            .any(|effect| effect["kind"] == "HiddenContent")
            && stderr.contains("hidden files")
    } else if required.contains("environment-file") {
        protected("Environment", false) && stderr.contains("environment")
    } else if required.contains("Zsh") || required.contains("zsh") {
        (gap("ExecutorDivergence") || gap("UnsupportedDialectConstruct"))
            && stderr.contains("unsupported")
    } else if required.contains("parse failure") || required.contains("redirection lacks") {
        gap("UnsupportedShellSyntax") && stderr.contains("shell syntax")
    } else if required.contains("execution owner") {
        gap("ExecutionOwnerUnavailable") && stderr.contains("execution owner")
    } else {
        false
    };
    assert!(
        matches,
        "{} required effect/failure {required}: effects={} coverage={} stderr={stderr}",
        row["id"], actual["observed_effects"], actual["coverage"]
    );
}

pub(super) fn assert_observers(row: &Value, actual: &Value) {
    let id = text(row, "id");
    let class = actual["class"].as_str().unwrap();
    let stderr = actual["stderr"].as_str().unwrap();
    if ["D", "UR", "UO", "F"].contains(&class) {
        assert_eq!(actual["effects"]["protected_access_count"], 0);
    }
    if class == "D" {
        assert!(!stderr.is_empty());
        if id.contains("appdata") || id.contains("alias") || id.contains("qualifier-protected") {
            assert!(stderr.contains("app-data"));
        }
        if id.contains("credential") {
            assert!(
                stderr.contains("credential or environment file")
                    || stderr.contains("private material in the named .ssh directory")
            );
        }
        if id.contains("env") && !id.contains("dump") {
            assert!(stderr.contains("environment"));
        }
        assert!(!stderr.contains("recovery:"));
        assert!(!actual["recovery"].is_null());
    }
    if class == "F" {
        let expected = row["reason_contract"]["effect_or_failure"]
            .as_str()
            .unwrap();
        assert_eq!(actual["coverage"]["error_kind"], expected);
        assert!(stderr.contains("recheck") && !stderr.contains("SYNTHETIC_CANARY"));
    }
    if class == "UR" {
        let excluded = actual["recovery"]["excluded_scope"].to_string();
        for (gap, required_scope) in [
            ("IdentityBound", "unresolved resource identity"),
            ("InspectionBudget", "over-budget function expansion"),
            ("UnsupportedShellSyntax", "original unsupported shell"),
        ] {
            if actual["coverage"]["gaps"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == gap)
            {
                assert!(
                    excluded.contains(required_scope),
                    "{id} recovery boundary {gap}"
                );
            }
        }
    }
    if id.contains("deadline") || id.contains("cancelled") {
        assert_eq!(actual["lifecycle"]["ready_receipt"], true);
        assert_eq!(actual["lifecycle"]["reaped"], true);
        assert_eq!(actual["lifecycle"]["completion_observed_by_wait"], true);
    }
    if id.contains("hidden-names") {
        assert!(
            actual["effects"]["names"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s == ".env")
        );
        assert_eq!(actual["effects"]["protected_access_count"], 0);
    }
    if id.contains("inline-config") || id.contains("runtime-config-protected") {
        assert_eq!(actual["effects"]["protected_access_count"], 1);
        assert_eq!(class, "UC");
    }
    if class == "A" && row["consumer"] == "claude" {
        let output: Value = serde_json::from_str(actual["stdout"].as_str().unwrap()).unwrap();
        let context = output["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap();
        assert_eq!(context.matches("rg -r means --replace.").count(), 1);
        assert_eq!(
            context,
            "rg -r means --replace. Drop -r; use -n for line numbers, or spell --replace VALUE for an intentional replacement."
        );
    }
    assert_recovery_observers(row, actual, id, class);
}

fn assert_recovery_observers(row: &Value, actual: &Value, id: &str, class: &str) {
    if row
        .get("recovery_objective")
        .is_some_and(|value| !value.is_null())
        && class != "F"
    {
        let recovery = &actual["recovery"];
        assert_eq!(recovery["automatic_application_supported"], false);
        assert!(
            recovery.get("objective").is_none(),
            "guard cannot claim agent intent"
        );
        assert_eq!(recovery["next_step"]["kind"], "owner_action");
        assert!(!recovery["preserved_scope"].as_array().unwrap().is_empty());
        if row["recovery_objective"]
            .get("next_operation")
            .is_some_and(|next| next.get("tool").is_some() && next.get("input").is_some())
        {
            assert_eq!(
                actual["recovery_receipt"]["state"], "rechecked",
                "{id} agent continuation"
            );
            assert_eq!(
                actual["recovery_receipt"]["agent_continuation_witness"],
                true
            );
            assert_eq!(
                actual["recovery_receipt"]["effects"]["protected_access_count"],
                0
            );
        } else {
            assert_eq!(actual["recovery_receipt"]["original_task_complete"], false);
        }
        if id.contains("search-home") || id.contains("shell-home") || id.contains("search-library")
        {
            let excluded = recovery["excluded_scope"].to_string();
            assert!(
                excluded.contains("HOME")
                    && excluded.contains("outside")
                    && excluded.contains("Library")
                    && excluded.contains(".ssh")
                    && excluded.contains("environment-file"),
                "{id} broad scope exclusions"
            );
            assert_eq!(
                actual["recovery_receipt"]["original_whole_home_complete"],
                false
            );
        }
    }
}
