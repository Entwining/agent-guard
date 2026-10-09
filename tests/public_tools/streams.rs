use super::*;

#[test]
fn supervision_preserves_separate_and_combined_streams_without_pipe_deadlock() {
    use std::sync::atomic::AtomicUsize;
    for merged in [false, true] {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "printf out; printf err >&2"]);
        let result = public_tools::execute(
            &mut command,
            &[],
            Duration::from_secs(2),
            Duration::from_millis(250),
            &AtomicUsize::new(0),
            merged,
        )
        .unwrap();
        assert_eq!(result.exit, 0);
        assert!(!result.timed_out);
        assert_eq!(result.stdout, if merged { "outerr" } else { "out" });
        assert_eq!(result.stderr, if merged { "" } else { "err" });
    }
    let input = vec![b'a'; 128 * 1024];
    let result = public_tools::execute(
        &mut Command::new("/bin/cat"),
        &input,
        Duration::from_secs(2),
        Duration::from_millis(250),
        &AtomicUsize::new(0),
        false,
    )
    .unwrap();
    assert_eq!(result.exit, 0);
    assert!(!result.timed_out);
    assert_eq!(result.stdout.as_bytes(), input);
    assert!(result.stderr.is_empty());
}
