use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const BIN: &str = env!("CARGO_BIN_EXE_itgmania-harness-rs");

#[test]
fn compares_parsed_values_and_reports_paths() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "itgmania-harness-rs-diff-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    let expected = directory.join("expected.json");
    let actual = directory.join("actual.json");
    std::fs::write(&expected, r#"{"number":1,"chart":{"meter":9}}"#).unwrap();
    std::fs::write(&actual, r#"{"number":1.0,"chart":{"meter":9}}"#).unwrap();

    let equal = run_diff(&expected, &actual);
    assert!(equal.status.success(), "{}", stderr(&equal));
    assert_eq!(
        String::from_utf8(equal.stdout).unwrap().trim(),
        "no differences"
    );

    std::fs::write(&actual, r#"{"number":1e0,"chart":{"meter":10}}"#).unwrap();
    let different = run_diff(&expected, &actual);
    assert!(!different.status.success());
    assert!(stderr(&different).contains("$.chart.meter: expected 9, actual 10"));

    std::fs::remove_dir_all(directory).unwrap();
}

fn run_diff(expected: &Path, actual: &Path) -> std::process::Output {
    Command::new(BIN)
        .args(["diff", expected.to_str().unwrap(), actual.to_str().unwrap()])
        .output()
        .unwrap()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
