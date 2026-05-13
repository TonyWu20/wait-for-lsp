use serde_json::Value;
use wait_for_lsp::config::Config;
use wait_for_lsp::filter::{filter_message, new_version_map};
use wait_for_lsp::parser::MessageParser;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn read_fixture(name: &str) -> Vec<u8> {
    let path = format!("tests/fixtures/{}", name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {}: {}", name, e))
}

fn parse_fixture(name: &str) -> Vec<Value> {
    let data = read_fixture(name);
    let mut parser = MessageParser::new();
    parser.feed(&data)
}

// ---------------------------------------------------------------------------
// Pyright fixture tests
// ---------------------------------------------------------------------------

#[test]
fn test_pyright_fixture_parses_all_messages() {
    let msgs = parse_fixture("pyright-session.bin");
    // From capture log: message 12 is publishDiagnostics, message 13 is shutdown
    // Let the parser tell us the exact count
    assert!(
        msgs.len() > 10,
        "Expected many messages from pyright session, got {}",
        msgs.len()
    );
    // Verify the last message is a shutdown response
    assert_eq!(msgs.last().unwrap()["id"], 2);
}

#[test]
fn test_pyright_fixture_has_diagnostics() {
    let msgs = parse_fixture("pyright-session.bin");
    let diag_msgs: Vec<&Value> = msgs
        .iter()
        .filter(|m| m["method"] == "textDocument/publishDiagnostics")
        .collect();
    assert_eq!(
        diag_msgs.len(),
        1,
        "Expected exactly 1 publishDiagnostics in pyright fixture"
    );
    let params = &diag_msgs[0]["params"];
    let diagnostics = params["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 3, "Expected 3 diagnostics from pyright");

    // Verify each diagnostic is severity 1 (Error)
    for d in diagnostics {
        assert_eq!(d["severity"], 1, "All pyright diagnostics should be severity 1");
    }

    // Verify the publishDiagnostics includes a version field
    assert_eq!(
        params["version"], 1,
        "Pyright should include version in publishDiagnostics"
    );

    // Verify the URI
    let uri = params["uri"].as_str().unwrap();
    assert!(uri.contains("main.py"), "URI should reference main.py");
}

#[test]
fn test_pyright_drop_all_filter() {
    let msgs = parse_fixture("pyright-session.bin");
    let config = Config {
        drop_diagnostics: true,
        min_severity: 1,
        stale_filter_enabled: false,
        log_enabled: false,
    };
    let vm = new_version_map();
    let filtered: Vec<Value> = msgs
        .iter()
        .filter_map(|m| filter_message(m, &config, &vm))
        .collect();

    // All non-diagnostic messages should survive
    // Only the single publishDiagnostics should be dropped
    let non_diag_count = msgs
        .iter()
        .filter(|m| m["method"] != "textDocument/publishDiagnostics")
        .count();
    assert_eq!(
        filtered.len(),
        non_diag_count,
        "With drop_all, all non-diag messages should survive"
    );

    // Verify no publishDiagnostics in filtered output
    for msg in &filtered {
        assert_ne!(
            msg["method"], "textDocument/publishDiagnostics",
            "publishDiagnostics should be dropped"
        );
    }
}

#[test]
fn test_pyright_non_diag_unchanged() {
    let msgs = parse_fixture("pyright-session.bin");
    let config = Config {
        drop_diagnostics: false,
        min_severity: 4, // Keep all
        stale_filter_enabled: false,
        log_enabled: false,
    };
    let vm = new_version_map();

    // window/logMessage messages should pass through unchanged
    for msg in &msgs {
        if msg["method"] == "window/logMessage" {
            let result = filter_message(msg, &config, &vm).unwrap();
            assert_eq!(result["method"], "window/logMessage");
            assert_eq!(result["params"]["message"], msg["params"]["message"]);
        }
    }
}

// ---------------------------------------------------------------------------
// Fortls fixture tests
// ---------------------------------------------------------------------------

#[test]
fn test_fortls_fixture_parses_all_messages() {
    let msgs = parse_fixture("fortls-session.bin");
    // fortls session: initialize response + publishDiagnostics (empty) + shutdown
    assert!(
        msgs.len() >= 3,
        "Expected at least 3 messages from fortls session, got {}",
        msgs.len()
    );

    // First message should be initialize response
    assert_eq!(msgs[0]["id"], 1);
    assert!(msgs[0].get("result").is_some());

    // Last message should be shutdown response
    assert_eq!(msgs.last().unwrap()["id"], 2);
}

#[test]
fn test_fortls_empty_diagnostics() {
    let msgs = parse_fixture("fortls-session.bin");
    let diag_msgs: Vec<&Value> = msgs
        .iter()
        .filter(|m| m["method"] == "textDocument/publishDiagnostics")
        .collect();

    assert_eq!(
        diag_msgs.len(),
        1,
        "Expected exactly 1 publishDiagnostics in fortls fixture"
    );

    let diagnostics = diag_msgs[0]["params"]["diagnostics"].as_array().unwrap();
    assert!(
        diagnostics.is_empty(),
        "fortls fixture should have empty diagnostics (fortls is a completion/navigation server)"
    );
}

#[test]
fn test_fortls_drop_all_filter() {
    let msgs = parse_fixture("fortls-session.bin");
    let config = Config {
        drop_diagnostics: true,
        min_severity: 1,
        stale_filter_enabled: false,
        log_enabled: false,
    };
    let vm = new_version_map();
    let filtered: Vec<Value> = msgs
        .iter()
        .filter_map(|m| filter_message(m, &config, &vm))
        .collect();

    // With drop_all: only the empty publishDiagnostics should be dropped
    let expected = msgs.len() - 1;
    assert_eq!(filtered.len(), expected);
}

#[test]
fn test_fortls_empty_diag_passes_when_not_dropped() {
    let msgs = parse_fixture("fortls-session.bin");
    let config = Config {
        drop_diagnostics: false,
        min_severity: 4,
        stale_filter_enabled: false,
        log_enabled: false,
    };
    let vm = new_version_map();

    // The empty publishDiagnostics should pass through unchanged
    for msg in &msgs {
        let result = filter_message(msg, &config, &vm).unwrap();
        if msg["method"] == "textDocument/publishDiagnostics" {
            let orig_diags = msg["params"]["diagnostics"].as_array().unwrap();
            let result_diags = result["params"]["diagnostics"].as_array().unwrap();
            assert_eq!(result_diags.len(), orig_diags.len());
            assert!(result_diags.is_empty());
        }
    }
}

// ---------------------------------------------------------------------------
// Multi-header handling (fortls sends Content-Type)
// ---------------------------------------------------------------------------

#[test]
fn test_fortls_header_has_content_type() {
    let data = read_fixture("fortls-session.bin");
    let header_end = data.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let header = std::str::from_utf8(&data[..header_end]).unwrap();

    // Assert that fortls includes a Content-Type header
    assert!(
        header.to_lowercase().contains("content-type:"),
        "Fortls fixture should have Content-Type header, got: {}",
        header
    );
    // And Content-Length header
    assert!(
        header.to_lowercase().contains("content-length:"),
        "Fortls fixture should have Content-Length header"
    );

    // Parse it with the real parser to verify correctness
    let mut parser = MessageParser::new();
    let msgs = parser.feed(&data);
    assert!(!msgs.is_empty(), "Parser should handle Content-Type header");
}
