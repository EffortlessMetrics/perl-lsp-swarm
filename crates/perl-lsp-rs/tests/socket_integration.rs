mod support;

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

struct ChildGuard {
    child: Child,
}

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self { child }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn reserve_local_port() -> Result<u16, Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

fn connect_with_deadline(port: u16) -> Result<TcpStream, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut last_error = None;

    while Instant::now() < deadline {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(stream) => return Ok(stream),
            Err(error) => {
                last_error = Some(error);
                thread::sleep(Duration::from_millis(50));
            }
        }
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        format!("timed out waiting for perl-lsp socket on 127.0.0.1:{port}: {last_error:?}"),
    )
    .into())
}

/// Integration test for TCP socket mode.
/// Spawns the LSP server in socket mode, connects, and verifies the initialize handshake.
#[test]
fn test_socket_connection() -> Result<(), Box<dyn std::error::Error>> {
    let bin_path = support::product_binary_path()?;
    let port = reserve_local_port()?;

    let child = Command::new(&bin_path)
        .arg("--socket")
        .arg("--port")
        .arg(port.to_string())
        .env("PERL_LSP_QUIET", "1")
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .spawn()?;
    let child = ChildGuard::new(child);

    // Connect to the server with timeout
    let stream = connect_with_deadline(port)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;

    // Clone stream for reading/writing - BufReader will own the read half
    let mut write_stream = stream.try_clone()?;

    // Send initialize request
    let request = r#"{"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"processId": null, "rootUri": null, "capabilities": {}}}"#;
    let message = format!("Content-Length: {}\r\n\r\n{}", request.len(), request);
    write_stream.write_all(message.as_bytes())?;
    write_stream.flush()?;

    // Read response
    let mut reader = BufReader::new(stream);

    // Read headers
    let mut content_length = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line == "\r\n" {
            break;
        }
        if line.starts_with("Content-Length: ") {
            content_length = line.trim()["Content-Length: ".len()..].parse()?;
        }
    }

    assert!(content_length > 0, "Content-Length should be positive");

    // Read body
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body)?;
    let response_str = String::from_utf8(body)?;

    // Validate response
    assert!(response_str.contains("\"result\""), "Response should contain result");
    assert!(response_str.contains("\"capabilities\""), "Response should contain capabilities");

    // Queue several required responses, then close only the client's write
    // half. EOF must let the scheduler drain accepted responses before the
    // server tears down the connection.
    for id in [10, 11, 12] {
        let request = format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"unknown/method\",\"params\":{{}}}}"
        );
        let message = format!("Content-Length: {}\r\n\r\n{}", request.len(), request);
        write_stream.write_all(message.as_bytes())?;
    }
    let shutdown_request = r#"{"jsonrpc":"2.0","id":2,"method":"shutdown","params":{}}"#;
    let message = format!("Content-Length: {}\r\n\r\n{}", shutdown_request.len(), shutdown_request);
    write_stream.write_all(message.as_bytes())?;
    write_stream.flush()?;
    write_stream.shutdown(Shutdown::Write)?;

    let mut response_ids = Vec::new();
    for _ in 0..32 {
        let mut content_length = None;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line)? == 0 {
                return Err(format!("socket EOF dropped queued responses: {response_ids:?}").into());
            }
            if line == "\r\n" {
                break;
            }
            if let Some(length) = line.strip_prefix("Content-Length: ") {
                content_length = Some(length.trim().parse::<usize>()?);
            }
        }
        let length = content_length.ok_or("queued response lacked Content-Length")?;
        let mut body = vec![0; length];
        reader.read_exact(&mut body)?;
        let response: serde_json::Value = serde_json::from_slice(&body)?;
        if let Some(id) = response.get("id").and_then(serde_json::Value::as_i64) {
            if response.get("method").is_some()
                || (response.get("result").is_none() && response.get("error").is_none())
            {
                return Err(format!("id-bearing frame was not a response: {response}").into());
            }
            response_ids.push(id);
            if response_ids.len() == 4 {
                break;
            }
        } else if response.get("result").is_some() || response.get("error").is_some() {
            return Err(format!("id-less response during EOF drain: {response}").into());
        }
    }
    if response_ids.len() != 4 {
        return Err(format!("EOF drain did not produce four responses: {response_ids:?}").into());
    }
    response_ids.sort_unstable();
    if response_ids != [2, 10, 11, 12] {
        return Err(format!("EOF drain returned unexpected response IDs: {response_ids:?}").into());
    }
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let mut length = None;
        while line != "\r\n" {
            if let Some(value) = line.strip_prefix("Content-Length: ") {
                length = Some(value.trim().parse::<usize>()?);
            }
            line.clear();
            if reader.read_line(&mut line)? == 0 {
                return Err("socket EOF truncated trailing frame".into());
            }
        }
        let length = length.ok_or("trailing frame lacked Content-Length")?;
        let mut body = vec![0; length];
        reader.read_exact(&mut body)?;
        let value: serde_json::Value = serde_json::from_slice(&body)?;
        if value.get("id").is_some()
            || value.get("result").is_some()
            || value.get("error").is_some()
        {
            return Err(format!("EOF drain produced an extra response: {value}").into());
        }
    }

    // Give server time to exit gracefully before force-killing
    std::thread::sleep(std::time::Duration::from_millis(100));
    drop(child);

    Ok(())
}

#[test]
fn stdio_required_response_failure_terminates_with_stdin_open()
-> Result<(), Box<dyn std::error::Error>> {
    let bin_path = support::product_binary_path()?;
    let child = Command::new(&bin_path)
        .env("PERL_LSP_QUIET", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut child = ChildGuard::new(child);
    let mut stdin = child.child.stdin.take().ok_or("stdio child stdin unavailable")?;
    let stdout = child.child.stdout.take().ok_or("stdio child stdout unavailable")?;

    let initialize = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"processId":null,"rootUri":null,"capabilities":{}}}"#;
    let frame = format!("Content-Length: {}\r\n\r\n{}", initialize.len(), initialize);
    stdin.write_all(frame.as_bytes())?;
    stdin.flush()?;

    let (response_tx, response_rx) = std::sync::mpsc::sync_channel::<
        Result<(serde_json::Value, std::process::ChildStdout), String>,
    >(1);
    let reader_thread = thread::spawn(move || -> Result<_, String> {
        let mut reader = BufReader::new(stdout);
        let mut content_length = None;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).map_err(|error| error.to_string())? == 0 {
                return Err("stdio server closed stdout before initialize response".to_string());
            }
            if line == "\r\n" {
                break;
            }
            if let Some(length) = line.strip_prefix("Content-Length: ") {
                content_length =
                    Some(length.trim().parse::<usize>().map_err(|error| error.to_string())?);
            }
        }
        let length = content_length.ok_or("initialize response lacked Content-Length")?;
        let mut response = vec![0; length];
        reader.read_exact(&mut response).map_err(|error| error.to_string())?;
        let response = serde_json::from_slice(&response).map_err(|error| error.to_string())?;
        let stdout = reader.into_inner();
        response_tx.send(Ok((response, stdout))).map_err(|error| error.to_string())?;
        Ok(())
    });
    let (response, stdout) = match response_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(result) => result.map_err(io::Error::other)?,
        Err(error) => {
            let _ = child.child.kill();
            let _ = child.child.wait();
            let _ = reader_thread.join();
            return Err(format!("timed out waiting for bounded initialize read: {error}").into());
        }
    };
    reader_thread
        .join()
        .map_err(|_| "stdio initialize reader panicked")?
        .map_err(io::Error::other)?;
    if response.get("id") != Some(&serde_json::json!(1)) || response.get("result").is_none() {
        return Err(format!("initialize did not succeed: {response}").into());
    }
    if child.child.try_wait()?.is_some() {
        return Err("stdio server exited before response-output failure probe".into());
    }

    // Closing only the stdout read end keeps stdin live while making the next
    // required response impossible to deliver through the production writer.
    drop(stdout);
    let unknown = r#"{"jsonrpc":"2.0","id":14168,"method":"unknown/method","params":{}}"#;
    let frame = format!("Content-Length: {}\r\n\r\n{}", unknown.len(), unknown);
    stdin.write_all(frame.as_bytes())?;
    stdin.flush()?;

    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            return Err("stdio server did not terminate after required response failure".into());
        }
        thread::sleep(Duration::from_millis(25));
    };
    if !status.success() && status.code().is_none() {
        return Err(format!("stdio server terminated by signal: {status}").into());
    }
    Ok(())
}
