use super::ExecuteCommandProvider;

/// Run/test/critic commands resolve their interpreter through
/// `WorkspaceConfig.perl_path`, which no user-facing channel writes. Advising
/// that setting told the reader to do something impossible (#5376); the
/// remediation must name only actions a user can perform.
#[test]
fn execute_command_perl_error_gives_remediation_the_user_can_act_on() {
    let message = ExecuteCommandProvider::unresolved_execute_command_perl_error("script.pl");

    assert!(
        !message.contains("perl.path"),
        "must not name an unsettable interpreter setting, got: {message}"
    );
    assert!(
        !message.contains("launch.json"),
        "language-server message must not send users to launch.json, got: {message}"
    );
    assert!(message.contains("PATH"), "must name PATH, got: {message}");
    assert!(message.contains("Install Perl"), "must name installing Perl, got: {message}");
    // The failing file is what tells the user which command died.
    assert!(message.contains("script.pl"), "must name the file, got: {message}");
    // Refusing an ambient interpreter is deliberate and stays stated.
    assert!(
        message.contains("refusing ambient fallback"),
        "must keep the fail-closed statement, got: {message}"
    );
}
