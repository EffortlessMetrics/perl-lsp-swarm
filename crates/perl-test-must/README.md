# perl-test-must

Dependency-free panic-on-failure extraction helpers for Rust tests.

## Use the right failure path

Use `Result` and `?` when setup or helper work should propagate an error:

```rust
fn load_fixture() -> Result<&'static str, &'static str> {
    Ok("source")
}

fn setup() -> Result<&'static str, &'static str> {
    let source = load_fixture()?;
    Ok(source)
}
```

Use `perl-test-must` where the test scenario asserts that a branch is
impossible:

```rust
use perl_test_must::{must, must_err, must_some_with};

let value: Result<i32, &str> = Ok(42);
assert_eq!(must(value), 42);

let item = must_some_with(
    Some("Example"),
    "the fixture declares the expected item",
);
assert_eq!(item, "Example");

let rejected: Result<(), &str> = Err("invalid fixture");
assert_eq!(must_err(rejected), "invalid fixture");
```

## Public API

| Helper | Required branch | Context-bearing counterpart |
| --- | --- | --- |
| `must` | `Result::Ok` | `must_with` |
| `must_some` | `Option::Some` | `must_some_with` |
| `must_err` | `Result::Err` | `must_err_with` |

The `_with` variants preserve an `expect`-style explanation. All six helpers
report the invocation through `#[track_caller]` and include relevant type and
unexpected-value evidence. Fully qualified type-name spelling is diagnostic
output, not a stable portable string contract.

`must` and `must_with` intentionally omit `#[must_use]`: a test may assert that
a side-effecting `Result<(), E>` succeeded and deliberately discard the unit
value. The `must_some*` and `must_err*` families return values that should
normally be consumed.

## Package boundary

This crate owns only Result/Option assertion-boundary extraction. It does not
own production error handling, fixtures, BDD, property testing, snapshots, TAP,
or test-runner execution. `perl-tdd-support` may expose compatibility re-exports
during its own migration, but direct `perl-test-must` imports are the canonical
owner path for these helpers.

## License

MIT OR Apache-2.0
