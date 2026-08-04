// Keep this regression fixture in the integration-test target so the normal
// `cargo test -p xtask --tests` CI route executes it.
include!("../src/tasks/merge_integration_fixture.rs");
