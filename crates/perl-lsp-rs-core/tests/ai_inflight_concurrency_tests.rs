//! Live-concurrency (`maxInflight`) contract for the AI backend (`#8300`).
//!
//! The invariant under test is that `maxInflight` bounds *simultaneously
//! active* backend requests. Before this contract existed, `maxInflight` was
//! passed to the token bucket as its burst allowance; a token is consumed at
//! dispatch and never returned, so N callers could each take a token and then
//! all remain in flight at once. These tests fail against that arrangement.

use perl_lsp_rs_core::providers::ai::{InflightGate, OpenAiConfig, OpenAiProvider, RateLimiter};
use perl_lsp_rs_core::providers::inline_completion::{
    BackendError, BackendRequest, InlineCompletionBackend, PreparedInlineCompletionContext,
    StreamControl,
};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

macro_rules! check {
    ($condition:expr $(,)?) => {
        if !($condition) {
            return Err(std::io::Error::other(format!(
                "condition failed: {}",
                stringify!($condition)
            ))
            .into());
        }
    };
    ($condition:expr, $message:expr $(,)?) => {
        if !($condition) {
            return Err(std::io::Error::other(format!($message)).into());
        }
    };
}

macro_rules! check_eq {
    ($left:expr, $right:expr $(,)?) => {
        let left = &$left;
        let right = &$right;
        if left != right {
            return Err(std::io::Error::other(format!(
                "equality failed: left={:?}, right={:?}",
                left, right
            ))
            .into());
        }
    };
    ($left:expr, $right:expr, $message:expr $(,)?) => {
        let left = &$left;
        let right = &$right;
        if left != right {
            return Err(std::io::Error::other(format!($message)).into());
        }
    };
}

fn backend_request() -> BackendRequest {
    BackendRequest {
        context: PreparedInlineCompletionContext {
            prefix: "my $x = ".to_string(),
            current_line: "my $x = ".to_string(),
            ..PreparedInlineCompletionContext::default()
        },
        max_output_tokens: 16,
        timeout_ms: 2_000,
    }
}

/// A provider pointed at a loopback endpoint, with a rate limiter generous
/// enough that it can never be the control under test.
fn provider(endpoint: &str, max_inflight: u32) -> OpenAiProvider {
    let mut config = OpenAiConfig::new(
        endpoint.to_string(),
        "gpt-4o-mini".to_string(),
        "test-key".to_string(),
        2_000,
    );
    config.local_model_mode = true;
    config.max_inflight = max_inflight;
    // A deliberately huge burst: if concurrency were still enforced by the
    // token bucket, this would let every request through at once.
    OpenAiProvider::new(config, Arc::new(RateLimiter::new(1_000.0, 1_000)))
}

/// Saturation must be decided *before* any network work.
///
/// The endpoint is a port with no listener, so any request that reaches
/// dispatch fails with a transport error. Observing `Saturated` instead proves
/// the permit is taken first and that a saturated gate costs no connection.
#[test]
fn saturated_gate_refuses_before_any_network_dispatch() -> Result<(), Box<dyn std::error::Error>> {
    // Bind then drop, so the port is almost certainly unused.
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.local_addr()?.port()
    };
    let provider = provider(&format!("http://127.0.0.1:{port}/v1/chat/completions"), 1);

    // Occupy the only slot.
    let held = provider.inflight().try_acquire().ok_or("the gate must admit the first holder")?;

    let outcome = provider.stream(&backend_request(), &mut |_| StreamControl::Continue);

    check!(
        matches!(outcome, Err(BackendError::Saturated)),
        "a saturated gate must refuse before dispatch, got: {outcome:?}"
    );

    drop(held);
    check_eq!(provider.inflight_counters().active, 0);
    Ok(())
}

/// Refusal must be immediate. `stream()` runs on the LSP's shared read-worker
/// pool, so a saturated gate that parked the caller would hold one of those
/// slots — degrading hover and definition — which is the problem `maxInflight`
/// exists to prevent.
#[test]
fn saturation_refuses_immediately_rather_than_parking_the_caller()
-> Result<(), Box<dyn std::error::Error>> {
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.local_addr()?.port()
    };
    let provider = provider(&format!("http://127.0.0.1:{port}/v1/chat/completions"), 1);
    let _held = provider.inflight().try_acquire().ok_or("the gate must admit the first holder")?;

    let started = Instant::now();
    let outcome = provider.stream(&backend_request(), &mut |_| StreamControl::Continue);
    let elapsed = started.elapsed();

    check!(matches!(outcome, Err(BackendError::Saturated)));
    check!(
        elapsed < Duration::from_millis(100),
        "a saturated request must return now, not occupy a read worker; took {elapsed:?}"
    );
    Ok(())
}

/// A terminal network error after admission still releases the permit.
#[test]
fn an_admitted_request_releases_after_closed_port_failure() -> Result<(), Box<dyn std::error::Error>>
{
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.local_addr()?.port()
    };
    let provider = provider(&format!("http://127.0.0.1:{port}/v1/chat/completions"), 1);

    let outcome = provider.stream(&backend_request(), &mut |_| StreamControl::Continue);

    check!(
        matches!(outcome, Err(BackendError::Transport(_)) | Err(BackendError::Timeout)),
        "an admitted request must finish with a network terminal error, got: {outcome:?}"
    );
    check_eq!(
        provider.inflight_counters().active,
        0,
        "a transport failure must still release the permit"
    );
    check_eq!(provider.inflight_counters().released, 1);
    Ok(())
}

/// The end-to-end invariant: with `maxInflight = 1`, two concurrent `stream()`
/// calls are never both inside the backend at once.
///
/// A loopback server accepts a connection and holds it without replying, so
/// the admitted request stays genuinely in flight while the second contends.
/// The server counts accepted connections: exactly one may arrive.
#[test]
fn max_inflight_one_admits_only_one_concurrent_backend_call()
-> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let accepted = Arc::new(AtomicU32::new(0));

    let accepted_worker = Arc::clone(&accepted);
    let server = thread::spawn(move || -> Result<(), std::io::Error> {
        // Poll rather than block: the whole point of the test is that the
        // second connection never arrives, so a blocking accept would hang the
        // suite instead of failing it.
        listener.set_nonblocking(true)?;
        let mut held = Vec::new();
        let deadline = Instant::now() + Duration::from_millis(1_500);
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    accepted_worker.fetch_add(1, Ordering::SeqCst);
                    stream.set_nonblocking(false)?;
                    stream.set_read_timeout(Some(Duration::from_millis(200)))?;
                    let mut buffer = [0_u8; 512];
                    match stream.read(&mut buffer) {
                        Ok(_) => {}
                        Err(error)
                            if matches!(
                                error.kind(),
                                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                            ) => {}
                        Err(error) => return Err(error),
                    }
                    // Hold the socket open without replying, so the admitted
                    // request stays genuinely in flight while the other thread
                    // contends for the single permit.
                    held.push(stream);
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(error),
            }
        }
        for mut stream in held {
            let _ = stream.write_all(b"HTTP/1.1 204 No Content\r\n\r\n");
        }
        Ok(())
    });

    let provider = Arc::new(provider(&format!("http://127.0.0.1:{port}/v1/chat/completions"), 1));
    let start = Arc::new(Barrier::new(2));
    let saturated = Arc::new(AtomicU32::new(0));

    let handles: Vec<_> = (0..2)
        .map(|_| {
            let provider = Arc::clone(&provider);
            let start = Arc::clone(&start);
            let saturated = Arc::clone(&saturated);
            thread::spawn(move || -> Result<(), std::io::Error> {
                start.wait();
                let outcome = provider.stream(&backend_request(), &mut |_| StreamControl::Continue);
                match outcome {
                    Err(BackendError::Saturated) => {
                        saturated.fetch_add(1, Ordering::SeqCst);
                    }
                    Ok(()) => {}
                    Err(BackendError::Transport(_)) | Err(BackendError::Timeout) => {}
                    Err(error) => {
                        return Err(std::io::Error::other(format!(
                            "unexpected backend error: {error:?}"
                        )));
                    }
                }
                Ok(())
            })
        })
        .collect();

    for handle in handles {
        handle.join().map_err(|_| std::io::Error::other("request worker panicked"))??;
    }
    server.join().map_err(|_| std::io::Error::other("loopback server panicked"))??;

    check_eq!(
        saturated.load(Ordering::SeqCst),
        1,
        "exactly one of two concurrent requests must be refused at maxInflight=1"
    );
    check_eq!(
        accepted.load(Ordering::SeqCst),
        1,
        "the refused request must never open a connection"
    );
    check_eq!(provider.inflight_counters().active, 0, "every permit must be released");
    if provider.inflight_counters().released != 1 {
        return Err(
            std::io::Error::other("the admitted permit must be released exactly once").into()
        );
    }
    check_eq!(provider.inflight_counters().peak_active, 1);
    Ok(())
}

/// A large rate-limit burst cannot raise the number of simultaneously active
/// requests above `maxInflight`. This is the specific conflation the issue
/// names: burst is a refill allowance, not a concurrency ceiling.
#[test]
fn a_large_rate_limit_burst_cannot_raise_live_concurrency() -> Result<(), Box<dyn std::error::Error>>
{
    let gate = InflightGate::new(2);
    let limiter = RateLimiter::new(1_000.0, 1_000);

    // Every caller can take a rate token...
    for _ in 0..8 {
        check!(limiter.try_acquire(), "the burst must be large enough to admit all callers");
    }

    // ...but only `maxInflight` may be live at once.
    let mut permits = Vec::new();
    for _ in 0..2 {
        let permit = gate.try_acquire();
        check!(permit.is_some());
        permits.push(permit);
    }
    check!(
        gate.try_acquire().is_none(),
        "rate-limit burst headroom must not increase the live-request ceiling"
    );
    check_eq!(gate.counters().peak_active, 2);
    Ok(())
}

/// Reconfiguring the profile builds a new provider. Permits outstanding on the
/// previous generation must neither block the new gate nor be lost by it.
#[test]
fn a_reconfigured_provider_does_not_share_or_strand_permits()
-> Result<(), Box<dyn std::error::Error>> {
    let old = provider("http://127.0.0.1:9/v1/chat/completions", 1);
    let held =
        old.inflight().try_acquire().ok_or("the old generation must admit its first request")?;
    check_eq!(old.inflight_counters().active, 1);

    // Profile replacement. Bind the permit: a temporary would drop inside the
    // assertion and the occupancy check below would read zero.
    let new = provider("http://127.0.0.1:9/v1/chat/completions", 1);
    let _fresh = new
        .inflight()
        .try_acquire()
        .ok_or("a permit outstanding on the retired generation must not consume new capacity")?;

    drop(held);
    check_eq!(
        old.inflight_counters().active,
        0,
        "the old permit must drain into the gate it came from"
    );
    check_eq!(new.inflight_counters().active, 1, "the new generation keeps its own occupancy");
    Ok(())
}
