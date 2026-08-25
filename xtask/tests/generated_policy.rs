use assert_cmd::Command;

#[test]
fn check_generated_is_exposed_by_xtask() {
    let mut command = Command::cargo_bin("xtask").expect("xtask binary");
    let output = command.arg("check-generated").arg("--help").output().expect("run");
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(help.contains("--mode"));
    assert!(help.contains("--json"));
}
