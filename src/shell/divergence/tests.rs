use super::*;
fn detect(source: &str, spans: &[Range<usize>]) -> Result<Detection, CheckError> {
    detect_lexed(
        source,
        spans,
        &super::super::lexer::Lexed::scan(source).unwrap(),
    )
}
#[test]
fn glob_position_rule() {
    assert!(!qualifier("a|b", false));
    assert!(!qualifier(".", true));
    assert!(!qualifier("x", true));
    assert!(!qualifier("extra", true));
    assert!(qualifier("+filter", true));
    assert!(qualifier("e:'cat input':", true));
    assert!(!qualifier("e:'cat input':", false));
}
#[test]
fn original_utf8_spans_are_checked() {
    assert!(detect("路径", std::slice::from_ref(&(1..2))).is_err());
    let inert = "printf '${(f)v}'";
    let active = "printf \"${(f)v}\"";
    assert!(
        !detect(inert, std::slice::from_ref(&(0..inert.len())))
            .unwrap()
            .divergent
    );
    assert!(
        detect(active, std::slice::from_ref(&(0..active.len())))
            .unwrap()
            .divergent
    );
}
#[test]
fn parameter_mask_uses_the_complete_nested_region() {
    let source = "echo ${v:-${(e)${name}}} tail";
    let found = detect(source, std::slice::from_ref(&(0..source.len()))).unwrap();
    assert_eq!(found.masked, "echo ${v:-_____________} tail");
    assert_eq!(found.evaluated_variables, ["${name}"]);
}
#[test]
fn modifier_detection_has_its_own_cause_and_span() {
    for modifier in ["~", "~~", "=", "==", "^", "^^"] {
        for quoted in [false, true] {
            let expansion = format!("${{{modifier}v}}");
            let source = if quoted {
                format!("echo \"{expansion}\"")
            } else {
                format!("echo {expansion}")
            };
            let start = source.find("${").unwrap();
            let found = detect(&source, std::slice::from_ref(&(0..source.len()))).unwrap();
            assert!(found.divergent, "D30 modifier cause: {source}");
            assert_eq!(
                found.parameter_spans,
                vec![start..start + expansion.len()],
                "D30 span: {source}"
            );
            assert!(found.code.is_empty() && found.evaluated_variables.is_empty());
        }
    }
    assert!(!detect("echo '${~v}'", &[]).unwrap().divergent);
    assert!(!detect("a=(x y)", &[]).unwrap().divergent);
    assert!(detect("cat =(echo public)", &[]).unwrap().divergent);
}
#[test]
fn parameter_pairing_ignores_braces_in_nested_code() {
    let source = "echo ${~v:-$(echo {)}";
    let found = detect(source, &[]).unwrap();
    assert!(
        found.divergent,
        "D30 modifier still has a complete source span"
    );
    assert_eq!(
        found.parameter_spans,
        std::iter::once(5..source.len()).collect::<Vec<_>>()
    );
}

#[test]
fn control_byte_before_name_does_not_create_assignment_prefix() {
    for byte in ['\r', '\u{000b}', '\u{000c}'] {
        let source = format!("printf '%s\\n' {byte}a=(x)#X");
        let lexical = super::super::lexer::Lexed::scan(&source).unwrap();
        assert!(!assignment_prefix(
            &source,
            source.find("=(").unwrap(),
            &lexical
        ));
    }
}
