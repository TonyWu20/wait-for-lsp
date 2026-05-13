use clap::Parser;

/// LSP proxy that filters stale textDocument/publishDiagnostics notifications.
///
/// Receives LSP messages from Claude Code, forwards them to the real LSP server,
/// and filters diagnostics from the server's responses before returning them.
#[derive(Parser)]
#[command(name = "wait-for-lsp", version, about)]
struct Args {
    /// LSP server command to proxy
    lsp_command: String,
    /// Arguments to pass to the LSP server
    lsp_args: Vec<String>,
}

fn main() {
    let args = Args::parse();

    let config = wait_for_lsp::config::Config::from_env();

    if config.log_enabled() {
        eprintln!(
            "[wait-for-lsp] config: drop={}, min_severity={}",
            config.drop_diagnostics, config.min_severity
        );
    }

    let exit_code = wait_for_lsp::proxy::run_proxy(&config, &args.lsp_command, &args.lsp_args);
    std::process::exit(exit_code);
}
