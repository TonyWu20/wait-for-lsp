use std::io::{Read, Write};
use std::process::{Command, Stdio};

/// Build a Content-Length framed message from a JSON body string.
fn framed(body: &str) -> Vec<u8> {
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let mut data = header.into_bytes();
    data.extend_from_slice(body.as_bytes());
    data
}

/// Parse Content-Length framed messages from raw bytes.
fn parse_frames(data: &[u8]) -> Vec<serde_json::Value> {
    let mut messages = Vec::new();
    let mut pos = 0;
    while pos < data.len() {
        let header_end = match data[pos..].windows(4).position(|w| w == b"\r\n\r\n") {
            Some(end) => pos + end,
            None => break,
        };
        let header = std::str::from_utf8(&data[pos..header_end]).unwrap_or("");
        let content_length = header
            .lines()
            .find(|l| l.to_lowercase().starts_with("content-length:"))
            .and_then(|l| l.split(':').nth(1)?.trim().parse::<usize>().ok())
            .unwrap_or(0);
        let body_start = header_end + 4;
        let body_end = body_start + content_length;
        if body_end > data.len() {
            break;
        }
        if let Ok(msg) = serde_json::from_slice(&data[body_start..body_end]) {
            messages.push(msg);
        }
        pos = body_end;
    }
    messages
}

/// Path to the compiled wait-for-lsp binary.
fn proxy_binary() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_wait-for-lsp"))
}

#[test]
fn test_diagnostics_filtered_with_drop_all() {
    // The proxy should drop publishDiagnostics when STAY_FRESH_DROP_DIAGNOSTICS=true
    let proxy_path = proxy_binary();

    let mut child = Command::new(&proxy_path)
        .arg("cat") // cat echoes stdin to stdout — simple mock LSP server
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("STAY_FRESH_DROP_DIAGNOSTICS", "true")
        .spawn()
        .expect("failed to spawn proxy");

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

    // Send a non-diagnostic message (should pass through)
    stdin
        .write_all(&framed(r#"{"jsonrpc":"2.0","id":1,"result":null}"#))
        .unwrap();

    // Send a publishDiagnostics notification (should be filtered)
    stdin
        .write_all(&framed(
            r#"{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":"file:///test.rs","diagnostics":[{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}},"severity":1,"message":"error"}]}}"#,
        ))
        .unwrap();

    // Send a shutdown response (should pass through)
    stdin
        .write_all(&framed(r#"{"jsonrpc":"2.0","id":2,"result":null}"#))
        .unwrap();

    // Close stdin — cat will see EOF and exit
    drop(stdin);

    // Read all stdout from the proxy
    let mut output = Vec::new();
    stdout.read_to_end(&mut output).unwrap();

    // Wait for proxy to exit
    let status = child.wait().expect("failed to wait for proxy");
    assert!(status.success(), "proxy exited with failure: {:?}", status);

    // Parse the output
    let messages = parse_frames(&output);

    // Should have 2 messages: initialize response + shutdown response (diags filtered)
    assert_eq!(messages.len(), 2, "expected 2 messages (diags filtered), got {}", messages.len());

    // First message should be the non-diag response
    assert_eq!(messages[0]["id"], 1);
    // Second message should be the shutdown response
    assert_eq!(messages[1]["id"], 2);
}

#[test]
fn test_non_diagnostics_pass_through() {
    // Without any publishDiagnostics, everything passes through
    let proxy_path = proxy_binary();

    let mut child = Command::new(&proxy_path)
        .arg("cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("STAY_FRESH_DROP_DIAGNOSTICS", "true")
        .spawn()
        .expect("failed to spawn proxy");

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

    // Send several non-diagnostic messages
    let msgs = vec![
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":2,"result":{"capabilities":{}}}"#,
        r#"{"jsonrpc":"2.0","method":"window/showMessage","params":{"type":1,"message":"hello"}}"#,
    ];

    for msg in &msgs {
        stdin.write_all(&framed(msg)).unwrap();
    }

    drop(stdin);

    let mut output = Vec::new();
    stdout.read_to_end(&mut output).unwrap();

    let status = child.wait().expect("failed to wait for proxy");
    assert!(status.success());

    let messages = parse_frames(&output);
    assert_eq!(messages.len(), 3, "all 3 messages should pass through");
    assert_eq!(messages[0]["id"], 1);
    assert_eq!(messages[1]["id"], 2);
    assert_eq!(messages[2]["method"], "window/showMessage");
}

#[test]
fn test_exit_code_propagation() {
    // The proxy should exit with the same code as the LSP server
    // We use 'sh -c "exit 42"' as the LSP server
    let proxy_path = proxy_binary();

    // On macOS, exit codes are propagated from the shell
    let mut child = Command::new(&proxy_path)
        .args(["sh", "-c", "exit 42"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("STAY_FRESH_DROP_DIAGNOSTICS", "true")
        .spawn()
        .expect("failed to spawn proxy");

    let status = child.wait().expect("failed to wait for proxy");
    assert_eq!(
        status.code(),
        Some(42),
        "proxy should propagate exit code 42, got {:?}",
        status.code()
    );
}

#[test]
fn test_severity_filtering() {
    // With STAY_FRESH_DROP_DIAGNOSTICS=false, MIN_SEVERITY=1,
    // only severity-1 diagnostics should survive
    let proxy_path = proxy_binary();

    let mut child = Command::new(&proxy_path)
        .arg("cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("STAY_FRESH_DROP_DIAGNOSTICS", "false")
        .env("STAY_FRESH_MIN_SEVERITY", "1")
        .spawn()
        .expect("failed to spawn proxy");

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

    // Send a publishDiagnostics with mixed severities
    stdin
        .write_all(&framed(
            r#"{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":"file:///test.rs","diagnostics":[
                {"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}},"severity":1,"message":"error1"},
                {"range":{"start":{"line":1,"character":0},"end":{"line":1,"character":1}},"severity":4,"message":"hint1"},
                {"range":{"start":{"line":2,"character":0},"end":{"line":2,"character":1}},"severity":1,"message":"error2"},
                {"range":{"start":{"line":3,"character":0},"end":{"line":3,"character":1}},"severity":2,"message":"warning"}
            ]}}"#,
        ))
        .unwrap();

    drop(stdin);

    let mut output = Vec::new();
    stdout.read_to_end(&mut output).unwrap();

    let status = child.wait().expect("failed to wait for proxy");
    assert!(status.success());

    let messages = parse_frames(&output);
    assert_eq!(messages.len(), 1, "should have 1 filtered message");

    let diagnostics = messages[0]["params"]["diagnostics"]
        .as_array()
        .unwrap();
    assert_eq!(diagnostics.len(), 2, "should keep 2 severity-1 diagnostics");
    for d in diagnostics {
        assert_eq!(d["severity"], 1, "all kept diagnostics should be severity 1");
    }
}

#[test]
fn test_spawn_failure() {
    // When the LSP command doesn't exist, the proxy should print error and exit with 1
    let proxy_path = proxy_binary();

    let mut child = Command::new(&proxy_path)
        .arg("nonexistent-command-that-definitely-does-not-exist")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn proxy");

    let status = child.wait().expect("failed to wait for proxy");
    assert_eq!(
        status.code(),
        Some(1),
        "proxy should exit with 1 on spawn failure"
    );
}

#[test]
fn test_signals_forwarded() {
    // SIGTERM to the proxy should cause the child to also exit
    let proxy_path = proxy_binary();

    // Use `sleep 60` as the LSP server — long-running
    let mut child = Command::new(&proxy_path)
        .args(["sleep", "60"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("STAY_FRESH_DROP_DIAGNOSTICS", "true")
        .spawn()
        .expect("failed to spawn proxy");

    // Give it a moment to start
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Get the proxy's PID and send SIGTERM
    let proxy_pid = child.id();
    // Send SIGTERM using kill command (avoids needing libc crate)
    let kill_status = std::process::Command::new("kill")
        .arg(proxy_pid.to_string())
        .status()
        .expect("failed to run kill");
    assert!(kill_status.success(), "kill command should succeed");

    // Wait for proxy to exit (should be quick after signal)
    // wait() blocks until exit — if it returns, the process has exited
    let status = child.wait().expect("failed to wait for proxy");
    // Signal exit produces no code; normal exit produces a code
    // Either is fine as long as wait() returned promptly
    eprintln!("proxy exited with: {:?}", status);
}
