# wait-for-lsp

LSP proxy that filters stale `textDocument/publishDiagnostics` notifications between Claude Code and a real LSP server, preventing unnecessary fix loops caused by diagnostics from the previous file state.

## Language

**Proxy**:
The `wait-for-lsp` binary that sits between Claude Code and a real LSP server, forwarding stdin/stderr verbatim and filtering stdout for stale diagnostics.
_Avoid_: Wrapper, middleware, adapter

**Stale Diagnostics**:
`textDocument/publishDiagnostics` notifications whose content reflects the *previous* file state, not the current one.
_Avoid_: Old diagnostics, outdated diagnostics

**Severity**:
The integer level on a diagnostic (1=Error, 2=Warning, 3=Info, 4=Hint), used by the filter to decide what to drop or keep.
_Avoid_: Level, priority

**Content-Length Framing**:
The HTTP-like wire format LSP uses: `Content-Length: <N>\r\n\r\n<JSON body>`.
_Avoid_: Header, delimiter

**Diagnostic Drop**:
The filter behavior when `STAY_FRESH_DROP_DIAGNOSTICS=true`: the entire `publishDiagnostics` notification is suppressed.
_Avoid_: Filter out, discard

**Plugin**:
A `plugin.json` file that tells Claude Code's LSP infrastructure how to invoke the proxy with a given language server.
_Avoid_: Extension, addon

## Relationships

- The **Proxy** receives LSP messages from Claude Code, forwards them to a real LSP server, and filters **Stale Diagnostics** from the server's responses before returning them to Claude Code.
- Each **Plugin** configures the **Proxy** with the command for a specific language server.
- The **Filter** uses **Severity** to decide when **Diagnostic Drop** is partial vs complete.

## Example dialogue

> **Dev:** "When the proxy receives a `publishDiagnostics` notification, does it always drop it?"
> **Domain expert:** "No — if `STAY_FRESH_DROP_DIAGNOSTICS` is `false`, it applies a **Severity** threshold: diagnostics at or below the threshold pass through, more severe ones are dropped."
