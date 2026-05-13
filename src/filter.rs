use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::config::Config;

/// Shared map from document URI to the latest version reported by the client.
pub type VersionMap = Arc<Mutex<HashMap<String, i64>>>;

/// Create an empty version map for use with [`filter_message`].
pub fn new_version_map() -> VersionMap {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Apply diagnostic filtering to a parsed LSP message.
///
/// `versions` tracks the latest `didOpen`/`didChange` version per URI.
/// When `config.stale_filter_enabled`, any `publishDiagnostics` whose
/// `params.version` is older than the tracked version is dropped (stale).
///
/// Returns `None` if the message should be dropped entirely.
/// Returns `Some(msg)` (possibly modified) if it should be forwarded.
pub fn filter_message(msg: &Value, config: &Config, versions: &VersionMap) -> Option<Value> {
    // Only filter textDocument/publishDiagnostics notifications
    if msg.get("method").and_then(|m| m.as_str()) != Some("textDocument/publishDiagnostics") {
        return Some(msg.clone());
    }

    if config.drop_diagnostics {
        return None;
    }

    // Version-based staleness check
    if config.stale_filter_enabled {
        let uri = msg
            .get("params")
            .and_then(|p| p.get("uri"))
            .and_then(|u| u.as_str());
        let msg_version = msg
            .get("params")
            .and_then(|p| p.get("version"))
            .and_then(|v| v.as_i64());
        if let (Some(uri), Some(msg_version)) = (uri, msg_version) {
            if let Ok(map) = versions.lock() {
                if let Some(&tracked) = map.get(uri) {
                    if msg_version < tracked {
                        // Stale — client has already sent a newer version
                        return None;
                    }
                }
            }
        }
    }

    // Severity-based filtering: keep diagnostics at or below the threshold
    let mut filtered = msg.clone();
    if let Some(diagnostics) = filtered
        .get_mut("params")
        .and_then(|p| p.get_mut("diagnostics"))
        .and_then(|d| d.as_array_mut())
    {
        diagnostics.retain(|d| {
            let sev = d.get("severity").and_then(|s| s.as_u64()).unwrap_or(1) as u8;
            sev <= config.min_severity
        });
    }

    Some(filtered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn make_msg(method: Option<&str>, diagnostics: Option<Vec<Value>>) -> Value {
        let mut msg = serde_json::json!({
            "jsonrpc": "2.0"
        });
        if let Some(m) = method {
            msg["method"] = serde_json::json!(m);
            msg["params"] = serde_json::json!({});
            if let Some(diags) = diagnostics {
                msg["params"]["diagnostics"] = Value::Array(diags);
            }
        } else {
            msg["id"] = serde_json::json!(1);
            msg["result"] = serde_json::json!(null);
        }
        msg
    }

    fn diag(severity: u64) -> Value {
        serde_json::json!({
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 0, "character": 1}
            },
            "severity": severity,
            "message": "test"
        })
    }

    fn cfg(drop: bool, min_sev: u8) -> Config {
        Config {
            drop_diagnostics: drop,
            min_severity: min_sev,
            stale_filter_enabled: false,
            log_enabled: false,
        }
    }

    // --- DROP_DIAGNOSTICS = true ---

    #[test]
    fn test_drop_diags_true_drops_publish_diagnostics() {
        let msg = make_msg(Some("textDocument/publishDiagnostics"), Some(vec![]));
        let result = filter_message(&msg, &cfg(true, 1), &new_version_map());
        assert!(result.is_none());
    }

    #[test]
    fn test_drop_diags_true_passes_non_diag() {
        let msg = make_msg(None, None);
        let result = filter_message(&msg, &cfg(true, 1), &new_version_map());
        assert!(result.is_some());
    }

    #[test]
    fn test_drop_diags_true_passes_other_notification() {
        let msg = make_msg(Some("window/showMessage"), None);
        let result = filter_message(&msg, &cfg(true, 1), &new_version_map());
        assert!(result.is_some());
    }

    #[test]
    fn test_drop_diags_true_passes_request() {
        let msg = make_msg(Some("workspace/diagnostic/refresh"), None);
        let result = filter_message(&msg, &cfg(true, 1), &new_version_map());
        assert!(result.is_some());
    }

    // --- DROP_DIAGNOSTICS = false, severity filtering ---

    #[test]
    fn test_severity_filter_keeps_at_threshold() {
        let diags = vec![diag(1), diag(2), diag(3), diag(4)];
        let msg = make_msg(Some("textDocument/publishDiagnostics"), Some(diags));
        let result = filter_message(&msg, &cfg(false, 2), &new_version_map()).unwrap();
        let kept = result["params"]["diagnostics"].as_array().unwrap();
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0]["severity"], 1);
        assert_eq!(kept[1]["severity"], 2);
    }

    #[test]
    fn test_severity_filter_keeps_all_at_four() {
        let diags = vec![diag(1), diag(4)];
        let msg = make_msg(Some("textDocument/publishDiagnostics"), Some(diags));
        let result = filter_message(&msg, &cfg(false, 4), &new_version_map()).unwrap();
        let kept = result["params"]["diagnostics"].as_array().unwrap();
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn test_severity_filter_keeps_only_sev_one() {
        let diags = vec![diag(1), diag(2), diag(3), diag(4)];
        let msg = make_msg(Some("textDocument/publishDiagnostics"), Some(diags));
        let result = filter_message(&msg, &cfg(false, 1), &new_version_map()).unwrap();
        let kept = result["params"]["diagnostics"].as_array().unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0]["severity"], 1);
    }

    #[test]
    fn test_no_severity_field_treated_as_one() {
        let mut d = diag(3);
        d.as_object_mut().unwrap().remove("severity");
        let msg = make_msg(Some("textDocument/publishDiagnostics"), Some(vec![d]));
        let result = filter_message(&msg, &cfg(false, 1), &new_version_map()).unwrap();
        let kept = result["params"]["diagnostics"].as_array().unwrap();
        assert_eq!(kept.len(), 1, "missing severity defaults to 1, should be kept");
    }

    #[test]
    fn test_empty_diagnostics_passes_through() {
        let msg = make_msg(Some("textDocument/publishDiagnostics"), Some(vec![]));
        let result = filter_message(&msg, &cfg(false, 1), &new_version_map()).unwrap();
        let diags = result["params"]["diagnostics"].as_array().unwrap();
        assert!(diags.is_empty());
    }

    #[test]
    fn test_non_diag_passes_unchanged() {
        let msg = make_msg(Some("window/showMessage"), None);
        let result = filter_message(&msg, &cfg(false, 1), &new_version_map());
        assert_eq!(result.unwrap(), msg);
    }

    // --- Version-based stale filtering ---

    fn cfg_stale(min_sev: u8) -> Config {
        Config {
            drop_diagnostics: false,
            min_severity: min_sev,
            stale_filter_enabled: true,
            log_enabled: false,
        }
    }

    fn make_diag_msg(uri: &str, version: i64, diagnostics: Vec<Value>) -> Value {
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": uri,
                "version": version,
                "diagnostics": diagnostics
            }
        })
    }

    fn tracked(uri: &str, version: i64) -> VersionMap {
        let vm = new_version_map();
        vm.lock().unwrap().insert(uri.to_string(), version);
        vm
    }

    #[test]
    fn test_stale_diagnostic_dropped_when_older() {
        let msg = make_diag_msg("file:///test.rs", 1, vec![diag(1)]);
        // Tracked version is 2, message version is 1 → stale, dropped
        let result = filter_message(&msg, &cfg_stale(4), &tracked("file:///test.rs", 2));
        assert!(result.is_none());
    }

    #[test]
    fn test_fresh_diagnostic_passes_when_matching() {
        let msg = make_diag_msg("file:///test.rs", 2, vec![diag(1)]);
        // Tracked version is 2, message version is 2 → fresh
        let result = filter_message(&msg, &cfg_stale(4), &tracked("file:///test.rs", 2));
        assert!(result.is_some());
    }

    #[test]
    fn test_fresh_diagnostic_passes_when_newer() {
        let msg = make_diag_msg("file:///test.rs", 3, vec![diag(1)]);
        // Tracked version is 2, message version is 3 → fresh (server ahead)
        let result = filter_message(&msg, &cfg_stale(4), &tracked("file:///test.rs", 2));
        assert!(result.is_some());
    }

    #[test]
    fn test_stale_filter_no_version_in_message_passes() {
        let msg = make_diag_msg("file:///test.rs", 1, vec![diag(1)]);
        let mut msg = msg;
        msg["params"].as_object_mut().unwrap().remove("version");
        let result = filter_message(&msg, &cfg_stale(4), &tracked("file:///test.rs", 2));
        assert!(result.is_some());
    }

    #[test]
    fn test_stale_filter_no_tracked_version_passes() {
        let msg = make_diag_msg("file:///test.rs", 1, vec![diag(1)]);
        let result = filter_message(&msg, &cfg_stale(4), &new_version_map());
        assert!(result.is_some());
    }

    #[test]
    fn test_stale_filter_disabled_passes_old_diag() {
        let msg = make_diag_msg("file:///test.rs", 1, vec![diag(1)]);
        let config = Config {
            drop_diagnostics: false,
            min_severity: 4,
            stale_filter_enabled: false,
            log_enabled: false,
        };
        let result = filter_message(&msg, &config, &tracked("file:///test.rs", 2));
        assert!(result.is_some());
    }

    #[test]
    fn test_stale_filter_different_file_passes() {
        let msg = make_diag_msg("file:///other.rs", 1, vec![diag(1)]);
        let result = filter_message(&msg, &cfg_stale(4), &tracked("file:///test.rs", 2));
        assert!(result.is_some());
    }

    #[test]
    fn test_stale_filter_severity_also_applied() {
        let msg = make_diag_msg("file:///test.rs", 2, vec![diag(1), diag(4)]);
        let result = filter_message(&msg, &cfg_stale(1), &tracked("file:///test.rs", 2));
        let filtered = result.unwrap();
        let kept = filtered["params"]["diagnostics"].as_array().unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0]["severity"], 1);
    }

    // --- Fixture-anchored tests ---

    #[test]
    fn test_fixture_drop_all() {
        let fixture = std::fs::read("tests/fixtures/rust-analyzer-session.bin").unwrap();
        let mut parser = crate::parser::MessageParser::new();
        let msgs = parser.feed(&fixture);
        assert_eq!(msgs.len(), 7);

        let config = cfg(true, 1);
        let vm = new_version_map();
        let filtered: Vec<_> = msgs.iter().filter_map(|m| filter_message(m, &config, &vm)).collect();

        // Messages 2-5 (indices 1-4) are publishDiagnostics — should be dropped
        // Only messages 1, 6, 7 (indices 0, 5, 6) should survive
        assert_eq!(filtered.len(), 3, "Only non-diagnostic messages survive");

        // Message 1: initialize response
        assert_eq!(filtered[0]["id"], 1);
        // Message 6: workspace/diagnostic/refresh
        assert_eq!(filtered[1]["method"], "workspace/diagnostic/refresh");
        // Message 7: shutdown response
        assert_eq!(filtered[2]["id"], 2);
    }

    #[test]
    fn test_fixture_severity_filter_msg3() {
        let fixture = std::fs::read("tests/fixtures/rust-analyzer-session.bin").unwrap();
        let mut parser = crate::parser::MessageParser::new();
        let msgs = parser.feed(&fixture);

        let config = cfg(false, 1);
        // Message 3 (index 2) has 2 diagnostics, both severity 1
        let result = filter_message(&msgs[2], &config, &new_version_map()).unwrap();
        let kept = result["params"]["diagnostics"].as_array().unwrap();
        assert_eq!(kept.len(), 2, "Both sev-1 diagnostics preserved");
        for d in kept {
            assert_eq!(d["severity"], 1);
        }
    }

    #[test]
    fn test_fixture_severity_filter_msg5() {
        let fixture = std::fs::read("tests/fixtures/rust-analyzer-session.bin").unwrap();
        let mut parser = crate::parser::MessageParser::new();
        let msgs = parser.feed(&fixture);

        // Message 5 (index 4) has 7 diagnostics: 4 sev 1, 3 sev 4
        let config = cfg(false, 1);
        let result = filter_message(&msgs[4], &config, &new_version_map()).unwrap();
        let kept = result["params"]["diagnostics"].as_array().unwrap();
        // With min_severity=1: keep only severity-1 diagnostics
        assert_eq!(kept.len(), 4, "4 severity-1 diagnostics kept, 3 severity-4 dropped");
        for d in kept {
            assert_eq!(d["severity"], 1);
        }
    }
}
