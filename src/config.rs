use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    /// When true, all textDocument/publishDiagnostics notifications are dropped.
    pub drop_diagnostics: bool,
    /// Maximum severity to keep when drop_diagnostics is false.
    /// 1=Error, 2=Warning, 3=Info, 4=Hint.
    pub min_severity: u8,
    /// When true, emit debug log via eprintln!.
    pub log_enabled: bool,
}

impl Config {
    pub fn from_env() -> Self {
        let drop_diagnostics = match env::var("STAY_FRESH_DROP_DIAGNOSTICS") {
            Ok(v) => v.trim().to_lowercase() != "false",
            Err(_) => true,
        };

        let min_severity = env::var("STAY_FRESH_MIN_SEVERITY")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(1);

        let log_enabled = match env::var("STAY_FRESH_LOG") {
            Ok(v) => v.trim().to_lowercase() == "true",
            Err(_) => false,
        };

        Config {
            drop_diagnostics,
            min_severity,
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

    #[test]
    fn test_defaults() {
        let c = Config::from_env();
        // These are the documented defaults
        assert!(c.drop_diagnostics);
        assert_eq!(c.min_severity, 1);
        assert!(!c.log_enabled);
    }

    #[test]
    fn test_drop_diagnostics_false() {
        unsafe { env::set_var("STAY_FRESH_DROP_DIAGNOSTICS", "false") };
        let c = Config::from_env();
        assert!(!c.drop_diagnostics);
        unsafe { env::remove_var("STAY_FRESH_DROP_DIAGNOSTICS") };
    }

    #[test]
    fn test_drop_diagnostics_arbitrary() {
        unsafe { env::set_var("STAY_FRESH_DROP_DIAGNOSTICS", "anything") };
        let c = Config::from_env();
        assert!(c.drop_diagnostics);
        unsafe { env::remove_var("STAY_FRESH_DROP_DIAGNOSTICS") };
    }

    #[test]
    fn test_min_severity_custom() {
        unsafe { env::set_var("STAY_FRESH_MIN_SEVERITY", "3") };
        let c = Config::from_env();
        assert_eq!(c.min_severity, 3);
        unsafe { env::remove_var("STAY_FRESH_MIN_SEVERITY") };
    }

    #[test]
    fn test_min_severity_invalid() {
        unsafe { env::set_var("STAY_FRESH_MIN_SEVERITY", "not-a-number") };
        let c = Config::from_env();
        assert_eq!(c.min_severity, 1);
        unsafe { env::remove_var("STAY_FRESH_MIN_SEVERITY") };
    }

    #[test]
    fn test_log_enabled_true() {
        unsafe { env::set_var("STAY_FRESH_LOG", "true") };
        let c = Config::from_env();
        assert!(c.log_enabled);
        unsafe { env::remove_var("STAY_FRESH_LOG") };
    }

    #[test]
    fn test_log_enabled_false() {
        unsafe { env::set_var("STAY_FRESH_LOG", "false") };
        let c = Config::from_env();
        assert!(!c.log_enabled);
        unsafe { env::remove_var("STAY_FRESH_LOG") };
    }
}
