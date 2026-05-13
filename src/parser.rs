use serde_json::Value;

pub struct MessageParser {
    buffer: Vec<u8>,
}

impl MessageParser {
    pub fn new() -> Self {
        MessageParser {
            buffer: Vec::new(),
        }
    }

    /// Feed raw bytes into the parser. Returns any complete LSP messages parsed.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<Value> {
        self.buffer.extend_from_slice(chunk);
        let mut messages = Vec::new();

        while let Some(header_end) = self.find_header_end() {

            // Parse Content-Length from header
            let content_length = match self.parse_content_length(header_end) {
                Some(len) => len,
                None => {
                    // Malformed header — skip past this \r\n\r\n and try again
                    self.buffer.drain(..header_end + 4);
                    continue;
                }
            };

            // Check if we have the complete body
            let body_start = header_end + 4;
            let body_end = body_start + content_length;

            if body_end > self.buffer.len() {
                break; // Need more data
            }

            // Extract and parse body
            let body = &self.buffer[body_start..body_end];
            match serde_json::from_slice::<Value>(body) {
                Ok(msg) => {
                    messages.push(msg);
                }
                Err(_) => {
                    // Invalid JSON — skip this message
                }
            }

            // Remove consumed bytes
            self.buffer.drain(..body_end);
        }

        messages
    }

    fn find_header_end(&self) -> Option<usize> {
        self.buffer
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
    }

    fn parse_content_length(&self, header_end: usize) -> Option<usize> {
        let header = std::str::from_utf8(&self.buffer[..header_end]).ok()?;
        for line in header.split("\r\n") {
            let lower = line.to_lowercase();
            if lower.starts_with("content-length:") {
                let value = line["content-length:".len()..].trim();
                return value.parse::<usize>().ok();
            }
        }
        None
    }
}

impl Default for MessageParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn framed_msg(body: &[u8]) -> Vec<u8> {
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        let mut frame = header.into_bytes();
        frame.extend_from_slice(body);
        frame
    }

    #[test]
    fn test_single_message() {
        let mut parser = MessageParser::new();
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;
        let data = framed_msg(body.as_bytes());

        let msgs = parser.feed(&data);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["id"], 1);
    }

    #[test]
    fn test_partial_header_then_complete() {
        let mut parser = MessageParser::new();
        let body = r#"{"jsonrpc":"2.0","id":1}"#;
        let frame = framed_msg(body.as_bytes());

        // Feed partial header
        let partial = &frame[..5]; // "Conte"
        let msgs = parser.feed(partial);
        assert!(msgs.is_empty());

        // Feed the rest
        let msgs = parser.feed(&frame[5..]);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["id"], 1);
    }

    #[test]
    fn test_partial_body_then_complete() {
        let mut parser = MessageParser::new();
        let body = r#"{"jsonrpc":"2.0","id":42}"#;
        let frame = framed_msg(body.as_bytes());

        // Feed header + partial body
        let header_end = frame.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        let partial_end = header_end + 5;
        let msgs = parser.feed(&frame[..partial_end]);
        assert!(msgs.is_empty());

        // Feed rest of body
        let msgs = parser.feed(&frame[partial_end..]);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["id"], 42);
    }

    #[test]
    fn test_multiple_messages_in_one_chunk() {
        let mut parser = MessageParser::new();
        let body1 = r#"{"jsonrpc":"2.0","id":1}"#;
        let body2 = r#"{"jsonrpc":"2.0","id":2}"#;
        let mut data = framed_msg(body1.as_bytes());
        data.extend_from_slice(&framed_msg(body2.as_bytes()));

        let msgs = parser.feed(&data);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["id"], 1);
        assert_eq!(msgs[1]["id"], 2);
    }

    #[test]
    fn test_case_insensitive_header() {
        let mut parser = MessageParser::new();
        let body = r#"{"jsonrpc":"2.0","id":1}"#;
        let header = format!("content-length: {}\r\n\r\n", body.len());
        let mut data = header.into_bytes();
        data.extend_from_slice(body.as_bytes());

        let msgs = parser.feed(&data);
        assert_eq!(msgs.len(), 1);
    }

    #[test]
    fn test_content_length_uppercase() {
        let mut parser = MessageParser::new();
        let body = r#"{"jsonrpc":"2.0","id":1}"#;
        let header = format!("CONTENT-LENGTH: {}\r\n\r\n", body.len());
        let mut data = header.into_bytes();
        data.extend_from_slice(body.as_bytes());

        let msgs = parser.feed(&data);
        assert_eq!(msgs.len(), 1);
    }

    #[test]
    fn test_whitespace_tolerant() {
        let mut parser = MessageParser::new();
        let body = r#"{"jsonrpc":"2.0","id":1}"#;
        let header = format!("Content-Length:  {}\r\n\r\n", body.len());
        let mut data = header.into_bytes();
        data.extend_from_slice(body.as_bytes());

        let msgs = parser.feed(&data);
        assert_eq!(msgs.len(), 1);
    }

    #[test]
    fn test_malformed_header_skipped() {
        let mut parser = MessageParser::new();
        // A non-LSP message followed by a valid one
        let garbage = b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let body = r#"{"jsonrpc":"2.0","id":1}"#;
        let mut data = garbage.to_vec();
        data.extend_from_slice(&framed_msg(body.as_bytes()));

        let msgs = parser.feed(&data);
        // Should skip the garbage and parse the valid message
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["id"], 1);
    }

    #[test]
    fn test_invalid_json_skipped() {
        let mut parser = MessageParser::new();
        let invalid_body = b"this is not json";
        let valid_body = r#"{"jsonrpc":"2.0","id":2}"#;
        let mut data = framed_msg(invalid_body);
        data.extend_from_slice(&framed_msg(valid_body.as_bytes()));

        let msgs = parser.feed(&data);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["id"], 2);
    }

    #[test]
    fn test_replay_fixture() {
        let fixture = std::fs::read("tests/fixtures/rust-analyzer-session.bin")
            .expect("Fixture file not found — run tests from project root");

        let mut parser = MessageParser::new();
        let msgs = parser.feed(&fixture);

        assert_eq!(msgs.len(), 7, "Expected 7 messages from fixture");

        // Verify message 1 is initialize response
        assert!(msgs[0].get("id").and_then(|v| v.as_i64()) == Some(1));
        assert!(msgs[0].get("result").is_some());

        // Messages 2-5 are publishDiagnostics
        for i in 1..=4 {
            assert_eq!(
                msgs[i]["method"],
                "textDocument/publishDiagnostics",
                "Message {} should be publishDiagnostics",
                i + 1
            );
        }

        // Verify diagnostic counts for each publishDiagnostics message
        assert_eq!(
            msgs[1]["params"]["diagnostics"].as_array().map(|a| a.len()),
            Some(0),
            "Message 2 should have 0 diagnostics"
        );
        assert_eq!(
            msgs[2]["params"]["diagnostics"].as_array().map(|a| a.len()),
            Some(2),
            "Message 3 should have 2 diagnostics"
        );
        assert_eq!(
            msgs[3]["params"]["diagnostics"].as_array().map(|a| a.len()),
            Some(4),
            "Message 4 should have 4 diagnostics"
        );
        assert_eq!(
            msgs[4]["params"]["diagnostics"].as_array().map(|a| a.len()),
            Some(7),
            "Message 5 should have 7 diagnostics"
        );

        // Message 6 is workspace/diagnostic/refresh
        assert_eq!(msgs[5]["method"], "workspace/diagnostic/refresh");

        // Message 7 is shutdown response
        assert!(msgs[6].get("id").and_then(|v| v.as_i64()) == Some(2));
    }

    #[test]
    fn test_content_length_values_from_fixture() {
        // Verify we can detect correct Content-Length values by parsing message boundaries
        let fixture = std::fs::read("tests/fixtures/rust-analyzer-session.bin")
            .expect("Fixture file not found");

        let mut parser = MessageParser::new();
        let msgs = parser.feed(&fixture);
        assert_eq!(msgs.len(), 7);

        // After parsing 7 messages, the buffer should be empty
        // (all bytes consumed)
        assert!(parser.buffer.is_empty());
    }

    #[test]
    fn test_partial_json_edge_cases() {
        // Feed empty chunk
        let mut parser = MessageParser::new();
        let msgs = parser.feed(b"");
        assert!(msgs.is_empty());

        // Feed just \r\n
        let msgs = parser.feed(b"\r\n");
        assert!(msgs.is_empty());

        // No header terminator, just text
        let msgs = parser.feed(b"Content-Length: 42");
        assert!(msgs.is_empty());
    }

    #[test]
    fn test_content_length_no_colon_space() {
        let mut parser = MessageParser::new();
        let body = r#"{"jsonrpc":"2.0","id":1}"#;
        // Without space after colon
        let header = format!("Content-Length:{}\r\n\r\n", body.len());
        let mut data = header.into_bytes();
        data.extend_from_slice(body.as_bytes());

        let msgs = parser.feed(&data);
        assert_eq!(msgs.len(), 1);
    }
}
