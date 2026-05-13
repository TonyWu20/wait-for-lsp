use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    /// When true, all textDocument/publishDiagnostics notifications are dropped.
    pub drop_diagnostics: bool,
    /// Maximum severity to keep when drop_diagnostics is false.
    /// 1=Error, 2=Warning, 3=Info, 4=Hint.
    pub min_severity: u8,
    /// When true, drop publishDiagnostics whose version is older than
    /// the last didOpen/didChange version (stale).
    pub stale_filter_enabled: bool,
    /// When true, emit debug log via eprintln!.
    pub log_enabled: bool,
}

impl Config {
    pub fn from_env() -> Self {
        Self::from_source(|key| env::var(key))
    }

    fn from_source(mut f: impl FnMut(&str) -> Result<String, env::VarError>) -> Self {
        let drop_diagnostics = match f("STAY_FRESH_DROP_DIAGNOSTICS") {
            Ok(v) => v.trim().to_lowercase() != "false",
            Err(_) => true,
        };

        let min_severity = f("STAY_FRESH_MIN_SEVERITY")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(1);

        let stale_filter_enabled = match f("STAY_FRESH_STALE_FILTER") {
            Ok(v) => v.trim().to_lowercase() != "false",
            Err(_) => true,
        };

        let log_enabled = match f("STAY_FRESH_LOG") {
            Ok(v) => v.trim().to_lowercase() == "true",
            Err(_) => false,
        };

        Config {
            drop_diagnostics,
            min_severity,
            stale_filter_enabled,
            log_enabled,
        }
    }

    pub fn log_enabled(&self) -> bool {
        self.log_enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_from(vars: &[(&str, &str)]) -> Config {
        let map: std::collections::HashMap<&str, &str> = vars.iter().copied().collect();
        Config::from_source(|key| map.get(key).map(|s| s.to_string()).ok_or(env::VarError::NotPresent))
    }

    #[test]
    fn test_defaults() {
        let c = config_from(&[]);
        assert!(c.drop_diagnostics);
        assert_eq!(c.min_severity, 1);
        assert!(c.stale_filter_enabled);
        assert!(!c.log_enabled);
    }

    #[test]
    fn test_drop_diagnostics_false() {
        let c = config_from(&[("STAY_FRESH_DROP_DIAGNOSTICS", "false")]);
        assert!(!c.drop_diagnostics);
    }

    #[test]
    fn test_drop_diagnostics_arbitrary() {
        let c = config_from(&[("STAY_FRESH_DROP_DIAGNOSTICS", "anything")]);
        assert!(c.drop_diagnostics);
    }

    #[test]
    fn test_min_severity_custom() {
        let c = config_from(&[("STAY_FRESH_MIN_SEVERITY", "3")]);
        assert_eq!(c.min_severity, 3);
    }

    #[test]
    fn test_min_severity_invalid() {
        let c = config_from(&[("STAY_FRESH_MIN_SEVERITY", "not-a-number")]);
        assert_eq!(c.min_severity, 1);
    }

    #[test]
    fn test_log_enabled_true() {
        let c = config_from(&[("STAY_FRESH_LOG", "true")]);
        assert!(c.log_enabled);
    }

    #[test]
    fn test_log_enabled_false() {
        let c = config_from(&[("STAY_FRESH_LOG", "false")]);
        assert!(!c.log_enabled);
    }

    #[test]
    fn test_stale_filter_disabled() {
        let c = config_from(&[("STAY_FRESH_STALE_FILTER", "false")]);
        assert!(!c.stale_filter_enabled);
    }

    #[test]
    fn test_stale_filter_default_true() {
        let c = config_from(&[]);
        assert!(c.stale_filter_enabled);
    }
}
