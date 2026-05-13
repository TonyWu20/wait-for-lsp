"""Capture real LSP wire-protocol traffic from pyright-langserver.

Spawns pyright-langserver, sends LSP messages (initialize, didOpen with a .py file),
captures stdout including diagnostics.

Usage: uv run python tests/fixtures/capture_pyright_traffic.py
"""

import json
import os
import subprocess
import sys
import time
import threading

FIXTURES_DIR = os.path.dirname(os.path.abspath(__file__))
WORKSPACE_DIR = os.path.join(FIXTURES_DIR, "lsp-workspace")
SAMPLE_PY = os.path.join(WORKSPACE_DIR, "src", "main.py")
OUTPUT_FILE = os.path.join(FIXTURES_DIR, "pyright-session.bin")
CAPTURE_LOG = os.path.join(FIXTURES_DIR, "capture-pyright-log.txt")


def make_content_length_msg(obj: dict) -> bytes:
    body = json.dumps(obj, ensure_ascii=False).encode("utf-8")
    header = f"Content-Length: {len(body)}\r\n\r\n".encode("ascii")
    return header + body


def main():
    with open(SAMPLE_PY) as f:
        py_content = f.read()

    messages = []

    # 1. Initialize request
    messages.append(make_content_length_msg({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "processId": os.getpid(),
            "rootUri": f"file://{WORKSPACE_DIR}",
            "capabilities": {
                "textDocument": {
                    "publishDiagnostics": {}
                }
            }
        }
    }))

    # 2. Initialized notification
    messages.append(make_content_length_msg({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }))

    # 3. didOpen to trigger diagnostics
    messages.append(make_content_length_msg({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": f"file://{SAMPLE_PY}",
                "languageId": "python",
                "version": 1,
                "text": py_content
            }
        }
    }))

    print("Spawning pyright-langserver...", file=sys.stderr)
    proc = subprocess.Popen(
        ["pyright-langserver", "--stdio"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

    for msg in messages:
        proc.stdin.write(msg)
        proc.stdin.flush()

    stdout_data = bytearray()

    def reader():
        while True:
            try:
                chunk = proc.stdout.read(4096)
                if not chunk:
                    break
                stdout_data.extend(chunk)
            except Exception:
                break

    reader_thread = threading.Thread(target=reader, daemon=True)
    reader_thread.start()

    print(f"Waiting for diagnostics...", file=sys.stderr)
    time.sleep(5.0)

    # Shutdown
    shutdown_msg = make_content_length_msg({
        "jsonrpc": "2.0", "id": 2, "method": "shutdown", "params": None
    })
    proc.stdin.write(shutdown_msg)
    proc.stdin.flush()
    time.sleep(0.5)

    exit_msg = make_content_length_msg({
        "jsonrpc": "2.0", "method": "exit", "params": None
    })
    proc.stdin.write(exit_msg)
    proc.stdin.flush()
    proc.stdin.close()

    time.sleep(1.0)
    reader_thread.join(timeout=5.0)

    stderr_data = proc.stderr.read()
    proc.wait(timeout=5)

    print(f"\npyright exit code: {proc.returncode}", file=sys.stderr)
    print(f"Total stdout: {len(stdout_data)} bytes", file=sys.stderr)
    print(f"Total stderr: {len(stderr_data)} bytes", file=sys.stderr)

    # Save raw binary capture
    with open(OUTPUT_FILE, "wb") as f:
        f.write(stdout_data)

    print(f"\nSaved capture to: {OUTPUT_FILE}", file=sys.stderr)

    # Parse and log
    with open(CAPTURE_LOG, "w") as log:
        log.write("=== CAPTURED LSP MESSAGES (pyright) ===\n\n")
        pos = 0
        msg_num = 0
        while pos < len(stdout_data):
            header_end = stdout_data.find(b"\r\n\r\n", pos)
            if header_end == -1:
                log.write(f"[Remaining {len(stdout_data)-pos} bytes: partial/corrupt]\n")
                break

            header_part = stdout_data[pos:header_end].decode("utf-8", errors="replace")
            log.write(f"--- Message {msg_num + 1} ---\n")
            log.write(f"Headers: {header_part}\n")

            content_length = 0
            for line in header_part.split("\r\n"):
                if line.lower().startswith("content-length:"):
                    content_length = int(line.split(":")[1].strip())

            body_start = header_end + 4
            body_end = body_start + content_length

            if body_end > len(stdout_data):
                log.write(f"[Truncated: expected {content_length}, got {len(stdout_data)-body_start}]\n")
                break

            body = stdout_data[body_start:body_end]
            try:
                parsed = json.loads(body)
                method = parsed.get("method", "(response/notification)")
                msg_id = parsed.get("id", "(no id)")
                log.write(f"Method: {method}, id: {msg_id}\n")

                if parsed.get("method") == "textDocument/publishDiagnostics":
                    params = parsed.get("params", {})
                    diags = params.get("diagnostics", [])
                    log.write(f"Diagnostics count: {len(diags)}\n")
                    for d in diags:
                        log.write(f"  - {d.get('message', '')[:100]} (severity: {d.get('severity')})\n")

                log.write(f"Body: {json.dumps(parsed, indent=2)[:2000]}\n")
            except json.JSONDecodeError as e:
                log.write(f"JSON parse error: {e}\n")
                log.write(f"Raw body: {body[:500]}\n")

            log.write("\n")
            pos = body_end
            msg_num += 1

        log.write(f"\nTotal messages: {msg_num}\n")

    if "textDocument/publishDiagnostics" in open(CAPTURE_LOG).read():
        print(f"\n*** SUCCESS: Got publishDiagnostics! ***", file=sys.stderr)
    else:
        print(f"\nWarning: No diagnostics in capture", file=sys.stderr)

    return 0 if len(stdout_data) > 0 else 1


if __name__ == "__main__":
    sys.exit(main())
