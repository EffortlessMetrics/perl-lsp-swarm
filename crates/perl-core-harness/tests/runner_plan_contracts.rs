//! Integration proof for target-driven runner plans and membership parity.

// These modules are shared verbatim with the `perl-core-harness-runner-plan` and
// `perl-core-harness-targets` binaries. This proof exercises the plan and parity surface,
// so the topology-drift and CLI-only items are legitimately unused here.
#[path = "../src/runner_plan/build.rs"]
mod build;
#[path = "../src/runner_plan/compare.rs"]
mod compare;
#[allow(dead_code)]
#[path = "../src/target_contracts/contract.rs"]
mod contract;
#[allow(dead_code)]
#[path = "../src/target_contracts/io.rs"]
mod io;
#[allow(dead_code)]
#[path = "../src/target_contracts/matrix.rs"]
mod matrix;
#[allow(dead_code)]
#[path = "../src/target_contracts/model.rs"]
mod model;
#[path = "../src/runner_plan/normalize.rs"]
mod normalize;
#[allow(dead_code)]
#[path = "../src/runner_plan/model.rs"]
mod runner_model;
#[path = "../src/runner_plan/tests.rs"]
mod runner_plan_tests;
