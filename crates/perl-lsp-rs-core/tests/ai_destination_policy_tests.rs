//! Integration tests for AI completion destination policy and credential binding.

use perl_lsp_rs_core::providers::ai::{
    credential_may_attach, validate_endpoint, validate_endpoint_with_resolver, OpenAiConfig,
    OpenAiProvider, RateLimiter,
};
use perl_lsp_rs_core::providers::inline_completion::{
    BackendRequest, InlineCompletionBackend, PreparedInlineCompletionContext, StreamChunk,
    StreamControl,
};
use perl_tdd_support::must_some;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, TcpListener};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn backend_request() -> BackendRequest {
    BackendRequest {
        context: PreparedInlineCompletionContext {
            prefix: "my $x = ".to_string(),
            current_line: "my $x = ".to_string(),
            previous_non_empty_line: None,
            current_function: None,
            current_package: None,
            variables: Vec::new(),
            imports: Vec::new(),
        },
        max_output_tokens: 16,
        timeout_ms: 2_000,
    }
}

#[test]
fn rejects_remote_http_endpoint() {
    let err = validate_endpoint("http://api.example.com/v1/chat/completions", false).unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains("HTTPS") || message.contains("disallowed") || message.contains("resolve"),
        "unexpected error: {message}"
    );
}

#[test]
fn accepts_loopback_https_without_local_model_mode() -> Result<(), Box<dyn std::error::Error>> {
    let approved = validate_endpoint("https://127.0.0.1:9/v1", false)?;
    assert_eq!(approved.host, "127.0.0.1");
    assert_eq!(approved.port, 9);
    Ok(())
}

#[test]
fn accepts_loopback_http_only_with_local_model_mode() -> Result<(), Box<dyn std::error::Error>> {
    let approved = validate_endpoint("http://127.0.0.1:9/v1", true)?;
    assert_eq!(approved.scheme, "http");
    Ok(())
}

#[test]
fn rejects_localhost_when_resolver_returns_private_address() {
    let err = validate_endpoint_with_resolver("http://localhost:8080/v1", true, &|_host, _port| {
        Ok(vec![IpAddr::V4(Ipv4Addr::new(192, 168, 0, 2))])
    })
    .unwrap_err();
    assert!(err.to_string().contains("loopback"));
}

#[test]
fn rejects_private_and_metadata_targets_via_injected_resolver() {
    for (url, ip) in [
        ("https://internal.example/v1", IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3))),
        ("https://metadata.example/v1", IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254))),
        ("https://[fd00::1]/v1", IpAddr::V6("fd00::1".parse().expect("valid ipv6 literal"))),
    ] {
        let err = validate_endpoint_with_resolver(url, false, &move |_host, _port| {
            Ok(vec![ip])
        })
        .unwrap_err();
        assert!(
            err.to_string().contains("disallowed"),
            "expected disallowed address for {url}, got {err}"
        );
    }
}

#[test]
fn accepts_bracketed_ipv6_loopback_with_explicit_port() -> Result<(), Box<dyn std::error::Error>> {
    let approved =
        validate_endpoint_with_resolver("https://[::1]:11434/v1", false, &|_host, _port| {
            Ok(vec![IpAddr::V6("::1".parse().expect("valid ipv6 literal"))])
        })?;
    assert_eq!(approved.host, "::1");
    assert_eq!(approved.port, 11434);
    Ok(())
}

#[test]
fn accepts_punycode_hostname_fixture() -> Result<(), Box<dyn std::error::Error>> {
    let approved = validate_endpoint_with_resolver(
        "https://xn--bcher-kva.example/v1",
        false,
        &|_host, _port| Ok(vec![IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))]),
    )?;
    assert_eq!(approved.host, "xn--bcher-kva.example");
    Ok(())
}

#[test]
fn redirect_target_fails_credential_binding() -> Result<(), Box<dyn std::error::Error>> {
    let approved =
        validate_endpoint_with_resolver("http://127.0.0.1:18080/v1", true, &|_host, _port| {
            Ok(vec![IpAddr::V4(Ipv4Addr::LOCALHOST)])
        })?;
    assert!(
        !credential_may_attach(&approved, "http://127.0.0.1:18081/v1"),
        "redirect port change must not inherit credentials"
    );
    assert!(
        !credential_may_attach(&approved, "https://attacker.example/v1"),
        "redirect host change must not inherit credentials"
    );
    Ok(())
}

#[test]
fn redirect_response_is_not_followed_and_does_not_reach_secondary_host(
) -> Result<(), Box<dyn std::error::Error>> {
    let redirect_listener = TcpListener::bind("127.0.0.1:0")?;
    let redirect_port = redirect_listener.local_addr()?.port();

    let secondary_listener = TcpListener::bind("127.0.0.1:0")?;
    secondary_listener.set_nonblocking(true)?;
    let secondary_port = secondary_listener.local_addr()?.port();
    let secondary_hit = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let secondary_hit_worker = Arc::clone(&secondary_hit);

    let redirect_worker =
        thread::spawn(move || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            let (mut stream, _) = redirect_listener.accept()?;
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            loop {
                let read = stream.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let request_text = String::from_utf8_lossy(&request);
            assert!(
                request_text.contains("super-secret-key"),
                "approved endpoint should receive credentials once"
            );

            let location = format!("http://127.0.0.1:{secondary_port}/stolen");
            let response =
                format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\n\r\n");
            stream.write_all(response.as_bytes())?;
            Ok(())
        });

    let secondary_worker =
        thread::spawn(move || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            if let Ok((_, _)) = secondary_listener.accept() {
                secondary_hit_worker.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            Ok(())
        });

    let endpoint = format!("http://127.0.0.1:{redirect_port}/v1/chat/completions");
    let provider = OpenAiProvider::new(
        OpenAiConfig {
            endpoint: endpoint.clone(),
            model: "test-model".to_string(),
            api_key: "super-secret-key".to_string(),
            api_key_header: "Authorization".to_string(),
            api_key_prefix: Some("Bearer".to_string()),
            timeout_ms: 2_000,
            local_model_mode: true,
        },
        Arc::new(RateLimiter::new(10.0, 10)),
    );

    let result =
        provider.stream(&backend_request(), &mut |_chunk: StreamChunk| StreamControl::Continue);
    assert!(result.is_err(), "redirect response must not be treated as SSE success");

    redirect_worker.join().expect("redirect worker panicked")?;
    thread::sleep(Duration::from_millis(100));
    secondary_worker.join().expect("secondary worker panicked")?;

    assert!(
        !secondary_hit.load(std::sync::atomic::Ordering::SeqCst),
        "redirect must not contact secondary host"
    );
    Ok(())
}

#[test]
fn stream_rejects_disallowed_endpoint_before_network_io() {
    let provider = OpenAiProvider::new(
        OpenAiConfig {
            endpoint: "https://10.0.0.8/v1/chat/completions".to_string(),
            model: "test-model".to_string(),
            api_key: "super-secret-key".to_string(),
            api_key_header: "Authorization".to_string(),
            api_key_prefix: Some("Bearer".to_string()),
            timeout_ms: 2_000,
            local_model_mode: false,
        },
        Arc::new(RateLimiter::new(10.0, 10)),
    );

    let err = must_some(
        provider
            .stream(&backend_request(), &mut |_chunk: StreamChunk| StreamControl::Continue)
            .err(),
    );
    let message = err.to_string();
    assert!(!message.contains("super-secret-key"), "must not leak api key");
    assert!(
        message.contains("disallowed") || message.contains("transport error"),
        "unexpected error: {message}"
    );
}

#[test]
fn transport_errors_do_not_echo_api_key() {
    let provider = OpenAiProvider::new(
        OpenAiConfig {
            endpoint: "http://127.0.0.1:1/v1/chat/completions".to_string(),
            model: "test-model".to_string(),
            api_key: "super-secret-key".to_string(),
            api_key_header: "Authorization".to_string(),
            api_key_prefix: Some("Bearer".to_string()),
            timeout_ms: 200,
            local_model_mode: true,
        },
        Arc::new(RateLimiter::new(10.0, 10)),
    );

    let err = must_some(
        provider
            .stream(&backend_request(), &mut |_chunk: StreamChunk| StreamControl::Continue)
            .err(),
    );
    let message = err.to_string();
    assert!(!message.contains("super-secret-key"));
    assert!(message.contains("<redacted>") || !message.contains("Bearer"));
}
