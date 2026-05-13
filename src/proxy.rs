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

    // Spawn I/O threads
    let stdin_handle = {
        let sig = Arc::clone(&sig_received);
        std::thread::spawn(move || {
            thread_forward_stdin(child_stdin, &sig);
        })
    };

    let stdout_handle = {
        let config = config.clone();
        std::thread::spawn(move || {
            thread_filter_stdout(child_stdout, &config);
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

fn thread_forward_stdin(mut child_stdin: impl Write, sig_received: &AtomicBool) {
    let mut buf = [0u8; 8192];
    let mut stdin = std::io::stdin();
    loop {
        // Stop reading stdin if signal received
        if sig_received.load(Ordering::SeqCst) {
            break;
        }

        match stdin.read(&mut buf) {
            Ok(0) => break, // EOF
            Ok(n) => {
                if let Err(e) = child_stdin.write_all(&buf[..n]) {
                    // Child stdin pipe closed — child likely died
                    if config_log_enabled() {
                        eprintln!("[wait-for-lsp] stdin write error: {}", e);
                    }
                    break;
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
) {
    let mut buf = [0u8; 8192];
    let mut parser = MessageParser::new();
    let mut stdout = std::io::stdout();

    loop {
        match child_stdout.read(&mut buf) {
            Ok(0) => break, // EOF — child pipe closed
            Ok(n) => {
                let messages = parser.feed(&buf[..n]);
                for msg in &messages {
                    if let Some(filtered) = filter_message(msg, config) {
                        let body = serde_json::to_string(&filtered).unwrap();
                        let header = format!("Content-Length: {}\r\n\r\n", body.len());
                        if let Err(e) = stdout.write_all(header.as_bytes()) {
                            if config_log_enabled() {
                                eprintln!("[wait-for-lsp] stdout write error: {}", e);
                            }
                            return;
                        }
                        if let Err(e) = stdout.write_all(body.as_bytes()) {
                            if config_log_enabled() {
                                eprintln!("[wait-for-lsp] stdout write error: {}", e);
                            }
                            return;
                        }
                    }
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
