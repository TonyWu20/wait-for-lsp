use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::config::Config;
use crate::filter::filter_message;
use crate::parser::MessageParser;

/// Run the proxy: spawn the real LSP server, forward I/O with filtering.
/// Returns the child's exit code.
pub fn run_proxy(config: &Config, lsp_command: &str, lsp_args: &[String]) -> i32 {
    if config.log_enabled() {
        eprintln!(
            "[wait-for-lsp] spawning: {} {}",
            lsp_command,
            lsp_args.join(" ")
        );
        eprintln!(
            "[wait-for-lsp] config: drop={}, min_severity={}",
            config.drop_diagnostics, config.min_severity
        );
    }

    let mut child = match spawn_child(lsp_command, lsp_args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[wait-for-lsp] failed to spawn '{}': {}", lsp_command, e);
            return 1;
        }
    };

    let child_stdin = child.stdin.take().expect("child stdin");
    let child_stdout = child.stdout.take().expect("child stdout");
    let child_stderr = child.stderr.take().expect("child stderr");

    // Signal flag — set by Ctrl+C handler
    let sig_received = Arc::new(AtomicBool::new(false));

    // Set up Ctrl+C handler using child PID (avoids signal-safety issues with Mutex)
    let child_pid = child.id();
    let sig = Arc::clone(&sig_received);
    if let Err(e) = ctrlc::set_handler(move || {
        sig.store(true, Ordering::SeqCst);
        // Use kill command in a separate process — signal-safe
        let _ = std::process::Command::new("kill")
            .arg(child_pid.to_string())
            .status();
    }) {
        eprintln!("[wait-for-lsp] warning: failed to set signal handler: {}", e);
    }

    // Shared version tracker — updated by stdin thread, read by stdout thread
    let versions = crate::filter::new_version_map();

    // Spawn I/O threads
    let stdin_handle = {
        let sig = Arc::clone(&sig_received);
        let versions = versions.clone();
        std::thread::spawn(move || {
            thread_forward_stdin(child_stdin, &sig, versions);
        })
    };

    let stdout_handle = {
        let config = config.clone();
        let versions = versions.clone();
        std::thread::spawn(move || {
            thread_filter_stdout(child_stdout, &config, versions);
        })
    };

    let stderr_handle = {
        std::thread::spawn(move || {
            thread_forward_stderr(child_stderr);
        })
    };

    // Wait for child to exit
    let exit_status = match child.wait() {
        Ok(status) => status,
        Err(e) => {
            eprintln!("[wait-for-lsp] error waiting for child: {}", e);
            // Try to get exit code from child id() equivalent
            return 1;
        }
    };

    // Join I/O threads (they should exit as pipes close)
    let _ = stdin_handle.join();
    let _ = stdout_handle.join();
    let _ = stderr_handle.join();

    if config.log_enabled() {
        eprintln!("[wait-for-lsp] child exited with: {:?}", exit_status);
    }

    exit_status.code().unwrap_or(1)
}

fn spawn_child(command: &str, args: &[String]) -> Result<Child, std::io::Error> {
    Command::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
}

fn thread_forward_stdin(
    mut child_stdin: impl Write,
    sig_received: &AtomicBool,
    versions: crate::filter::VersionMap,
) {
    let mut buf = [0u8; 8192];
    let mut stdin = std::io::stdin();
    let mut version_parser = MessageParser::new();
    loop {
        // Stop reading stdin if signal received
        if sig_received.load(Ordering::SeqCst) {
            break;
        }

        match stdin.read(&mut buf) {
            Ok(0) => break, // EOF
            Ok(n) => {
                // Forward raw bytes to the child LSP server
                if let Err(e) = child_stdin.write_all(&buf[..n]) {
                    if config_log_enabled() {
                        eprintln!("[wait-for-lsp] stdin write error: {}", e);
                    }
                    break;
                }
                // Also parse for didOpen/didChange version tracking
                let msgs = version_parser.feed(&buf[..n]);
                for msg in &msgs {
                    if let Some(method) = msg.get("method").and_then(|m| m.as_str()) {
                        if method == "textDocument/didOpen" || method == "textDocument/didChange" {
                            if let (Some(uri), Some(version)) = (
                                msg.get("params")
                                    .and_then(|p| p.get("textDocument"))
                                    .and_then(|td| td.get("uri"))
                                    .and_then(|u| u.as_str()),
                                msg.get("params")
                                    .and_then(|p| p.get("textDocument"))
                                    .and_then(|td| td.get("version"))
                                    .and_then(|v| v.as_i64()),
                            ) {
                                if let Ok(mut map) = versions.lock() {
                                    if config_log_enabled() {
                                        eprintln!(
                                            "[wait-for-lsp] tracked {} version {}",
                                            uri, version
                                        );
                                    }
                                    map.insert(uri.to_string(), version);
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                if config_log_enabled() {
                    eprintln!("[wait-for-lsp] stdin read error: {}", e);
                }
                break;
            }
        }
    }
}

fn thread_filter_stdout(
    mut child_stdout: impl Read,
    config: &Config,
    versions: crate::filter::VersionMap,
) {
    let mut buf = [0u8; 8192];
    let mut parser = MessageParser::new();
    let mut stdout = std::io::stdout();
    // Per-URI dedup queue: only forward the latest publishDiagnostics per cycle
    let mut pending: std::collections::HashMap<String, serde_json::Value> = std::collections::HashMap::new();

    loop {
        match child_stdout.read(&mut buf) {
            Ok(0) => break, // EOF — child pipe closed
            Ok(n) => {
                let messages = parser.feed(&buf[..n]);
                for msg in &messages {
                    match filter_message(msg, config, &versions) {
                        Some(filtered) => {
                            // Check if this is a publishDiagnostics message
                            if filtered.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics") {
                                // Extract URI for dedup
                                if let Some(uri) = filtered
                                    .get("params")
                                    .and_then(|p| p.get("uri"))
                                    .and_then(|u| u.as_str())
                                {
                                    // Queue the latest — overwrite previous entry for this URI
                                    if config_log_enabled() {
                                        let ver = filtered
                                            .get("params")
                                            .and_then(|p| p.get("version"))
                                            .and_then(|v| v.as_i64());
                                        eprintln!(
                                            "[wait-for-lsp] queued {} version {:?}",
                                            uri, ver
                                        );
                                    }
                                    pending.insert(uri.to_string(), filtered);
                                    continue; // Don't forward yet — flush at end of cycle
                                }
                            }
                            // Non-diagnostic messages forward immediately
                            write_message(&filtered, &mut stdout);
                        }
                        None => {
                            // Message was filtered out (drop/severity/stale)
                            if config_log_enabled() {
                                if let Some(method) = msg.get("method").and_then(|m| m.as_str()) {
                                    if method == "textDocument/publishDiagnostics" {
                                        let uri = msg
                                            .get("params")
                                            .and_then(|p| p.get("uri"))
                                            .and_then(|u| u.as_str())
                                            .unwrap_or("<unknown>");
                                        let ver = msg
                                            .get("params")
                                            .and_then(|p| p.get("version"))
                                            .and_then(|v| v.as_i64());
                                        eprintln!(
                                            "[wait-for-lsp] dropped {} version {:?}",
                                            uri, ver
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                // Flush pending diagnostics: forward the latest per URI
                for (_uri, msg) in pending.drain() {
                    write_message(&msg, &mut stdout);
                }
                let _ = stdout.flush();
            }
            Err(e) => {
                if config_log_enabled() {
                    eprintln!("[wait-for-lsp] stdout read error: {}", e);
                }
                break;
            }
        }
    }
}

/// Write a framed LSP message to the given output stream.
fn write_message(msg: &serde_json::Value, mut out: impl Write) {
    let body = serde_json::to_string(msg).unwrap();
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let _ = out.write_all(header.as_bytes());
    let _ = out.write_all(body.as_bytes());
}

fn thread_forward_stderr(mut child_stderr: impl Read) {
    let mut buf = [0u8; 8192];
    let mut stderr = std::io::stderr();

    loop {
        match child_stderr.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if let Err(e) = stderr.write_all(&buf[..n]) {
                    if config_log_enabled() {
                        eprintln!("[wait-for-lsp] stderr write error: {}", e);
                    }
                    break;
                }
            }
            Err(e) => {
                if config_log_enabled() {
                    eprintln!("[wait-for-lsp] stderr read error: {}", e);
                }
                break;
            }
        }
    }
}

fn config_log_enabled() -> bool {
    // Quick check without Config reference
    std::env::var("STAY_FRESH_LOG").ok().is_some_and(|v| v.trim().to_lowercase() == "true")
}
