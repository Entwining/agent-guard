#[test]
fn quoted_class_delimiters_and_members_remain_literal() {
    for (pattern, subject, expected) in [
        (r"[a\]]", "]", true),
        (r"[a\]]", "a", true),
        (r"[\-x]", "-", true),
        (r"[\-x]", "k", false),
        (r"[\!a]", "b", false),
        (r"[\!a]", "!", true),
        (r"[\^a]", "^", true),
        (r"[\^a]", "b", false),
    ] {
        assert_eq!(
            super::component(pattern, subject),
            expected,
            "{pattern} {subject}"
        );
    }
}
#[test]
fn literal_subject_work_grows_with_width_and_pattern_depth() {
    for width in [16, 32, 64, 128] {
        for depth in [1, 2, 4, 8, 16] {
            let pattern = format!("{}public", "*x".repeat(depth));
            let subject = format!("{}{}public", "y".repeat(width), "x".repeat(depth));
            let mut comparisons = 0;
            assert!(super::component_counted(&pattern, &subject, &mut || {
                comparisons += 1
            }));
            assert!(
                comparisons <= 2 * (width + depth + 6),
                "width={width}, depth={depth}, comparisons={comparisons}"
            );
        }
    }
}

#[test]
fn literal_subject_matching_avoids_the_pattern_product() {
    for size in [64, 128, 256, 512] {
        let subject = format!("{}public", "x".repeat(size));
        let mut comparisons = 0;
        assert!(super::component_counted("*public", &subject, &mut || {
            comparisons += 1
        }));
        assert!(
            comparisons <= subject.chars().count() + 7,
            "size={size}, comparisons={comparisons}"
        );
    }
}
#[test]
fn literal_subject_matching_preserves_pattern_intersection_results() {
    let units = [
        "a",
        "b",
        "*",
        "?",
        "[ab]",
        "[!a]",
        "[a-c]",
        "[[:digit:]]",
        "\\*",
        "[",
        "\\",
        "é",
    ];
    let mut patterns = vec![String::new()];
    patterns.extend(units.iter().map(|s| s.to_string()));
    for first in units {
        for second in units {
            patterns.push(format!("{first}{second}"));
        }
    }
    let mut subjects = vec![String::new()];
    for width in 1..=3 {
        let alphabet = ['a', 'b', 'x', '*', '\\', 'é', '1'];
        for mut index in 0..alphabet.len().pow(width) {
            let mut subject = String::new();
            for _ in 0..width {
                subject.push(alphabet[index % alphabet.len()]);
                index /= alphabet.len();
            }
            subjects.push(subject);
        }
    }
    for pattern in patterns {
        for subject in &subjects {
            assert_eq!(
                super::component(&pattern, subject),
                super::intersects(&pattern, &super::escape_literal(subject)),
                "pattern={pattern:?}, subject={subject:?}"
            );
        }
    }
}
#[test]
fn incompatible_path_anchors_skip_parent_states() {
    for size in [8, 16, 32, 64] {
        let subject = format!("/public/{}/data.json", vec!["nested"; size].join("/"));
        let mut matcher = super::Matcher::default();
        assert!(!matcher.path("**/public.pem", &subject));
        assert!(!matcher.path(&subject, "/h/Library/Containers/x"));
        assert_eq!(matcher.path_states, 0, "size={size}");
    }
}
#[test]
fn universal_component_does_not_enumerate_subject_states() {
    for size in [64, 128, 256, 512] {
        let subject = "public路径[*]".repeat(size);
        let mut comparisons = 0;
        for pattern in ["*", "**", "***"] {
            assert!(super::component_counted(pattern, &subject, &mut || {
                comparisons += 1;
            }));
        }
        assert_eq!(comparisons, 0, "size={size}");
        assert!(!super::component_counted("", &subject, &mut || {}));
    }
}
#[test]
fn incompatible_pattern_anchors_skip_product_states() {
    for size in [64, 128, 256, 512] {
        let pattern = format!("public{}*[0-9].json", "data".repeat(size));
        let mut comparisons = 0;
        for protected in ["*.pem", "*.key", ".env*", "credentials*"] {
            assert!(!super::intersects_counted(&pattern, protected, &mut || {
                comparisons += 1;
            }));
        }
        assert_eq!(comparisons, 0, "size={size}");
    }
}
#[test]
fn incompatible_literal_anchors_skip_subject_states() {
    for size in [64, 128, 256, 512] {
        let subject = format!("{}.json", "public".repeat(size));
        let mut comparisons = 0;
        for pattern in ["*.pem", "*.key", ".env*", "config[0-9]*"] {
            assert!(!super::component_counted(pattern, &subject, &mut || {
                comparisons += 1;
            }));
        }
        assert!(comparisons <= 12, "size={size}, comparisons={comparisons}");
        let long_pattern = format!("public{}*.json", "data".repeat(size));
        let mut comparisons = 0;
        for subject in [".env", "containers", "credentials"] {
            assert!(!super::component_counted(
                &long_pattern,
                subject,
                &mut || {
                    comparisons += 1;
                }
            ));
        }
        assert_eq!(comparisons, 0, "size={size}");
    }
}
#[test]
fn pattern_state_walk_observes_its_deadline() {
    let expired = std::time::Instant::now() - std::time::Duration::from_secs(1);
    assert_eq!(
        super::Matcher::default()
            .path_checked("*public", "x", Some(expired))
            .unwrap_err()
            .kind,
        crate::CheckErrorKind::Deadline
    );
}
#[test]
fn literal_intersection_work_tracks_pattern_length() {
    for size in [64, 128, 256] {
        let left = format!("{}[*].id", "m".repeat(size));
        let right = format!("{}*.id", "m".repeat(size));
        let mut comparisons = 0;
        assert!(super::intersects_counted(&left, &right, &mut || {
            comparisons += 1
        }));
        assert!(
            comparisons <= 4 * (left.len() + right.len()),
            "size={size}, comparisons={comparisons}"
        );
    }
}
