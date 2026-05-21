use regex::Regex;
use std::borrow::Cow;

/// One-shot cleanup: remove `<memory-context>...</memory-context>` blocks from text.
pub fn sanitize_context(text: &str) -> Cow<'_, str> {
    let re = memory_context_regex();
    lazy_static_regex_replace(&re, text)
}

fn lazy_static_regex_replace<'a>(re: &Regex, text: &'a str) -> Cow<'a, str> {
    re.replace_all(text, "")
}

/// Returns the compiled regex for `<memory-context>...</memory-context>` blocks.
fn memory_context_regex() -> Regex {
    Regex::new(r"(?s)<memory-context>.*?</memory-context>").expect("invalid regex")
}

/// A streaming context scrubber that removes `<memory-context>` blocks
/// from text as it is being streamed in chunks.
///
/// This is useful for cleaning up provider-injected context blocks that
/// should not be shown to the end user during streaming.
pub struct StreamingContextScrubber {
    /// Buffer for partial matches that span chunk boundaries.
    buffer: String,
    /// Whether we are currently inside a `<memory-context>` block.
    inside_block: bool,
}

impl StreamingContextScrubber {
    /// Create a new streaming scrubber.
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            inside_block: false,
        }
    }

    /// Process a chunk of streamed text and return cleaned output.
    ///
    /// Returns `None` if the entire chunk was consumed by a blocked region.
    pub fn process_chunk(&mut self, chunk: &str) -> Option<String> {
        self.buffer.push_str(chunk);
        let mut output = String::new();

        loop {
            if self.inside_block {
                // Look for the closing tag
                if let Some(end_pos) = self.buffer.find("</memory-context>") {
                    let after = end_pos + "</memory-context>".len();
                    self.buffer = self.buffer[after..].to_string();
                    self.inside_block = false;
                    // Continue processing remaining buffer
                    continue;
                } else {
                    // Still inside the block, discard everything in buffer
                    self.buffer.clear();
                    break;
                }
            } else {
                // Look for the opening tag
                if let Some(start_pos) = self.buffer.find("<memory-context>") {
                    // Output everything before the tag
                    output.push_str(&self.buffer[..start_pos]);
                    let after = start_pos + "<memory-context>".len();
                    self.buffer = self.buffer[after..].to_string();
                    self.inside_block = true;
                    // Continue processing to find the end
                    continue;
                } else {
                    // No opening tag found; check if buffer ends with a partial tag.
                    // The longest partial prefix of "<memory-context>" that the
                    // buffer could end with.
                    let partial = find_partial_match(&self.buffer, "<memory-context>");
                    if partial > 0 {
                        // Keep the potential partial match in the buffer;
                        // output everything before it.
                        let safe_len = self.buffer.len() - partial;
                        output.push_str(&self.buffer[..safe_len]);
                        self.buffer = self.buffer[safe_len..].to_string();
                    } else {
                        // No partial match — flush the entire buffer.
                        output.push_str(&self.buffer);
                        self.buffer.clear();
                    }
                    break;
                }
            }
        }

        if output.is_empty() {
            None
        } else {
            Some(output)
        }
    }

    /// Flush any remaining buffered content.
    pub fn flush(&mut self) -> Option<String> {
        if self.buffer.is_empty() {
            None
        } else {
            let out = self.buffer.clone();
            self.buffer.clear();
            Some(out)
        }
    }

    /// Reset the scrubber to its initial state.
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.inside_block = false;
    }
}

impl Default for StreamingContextScrubber {
    fn default() -> Self {
        Self::new()
    }
}

/// Find the length of the longest suffix of `text` that is a prefix of `pattern`.
fn find_partial_match(text: &str, pattern: &str) -> usize {
    let pattern_bytes = pattern.as_bytes();
    let text_bytes = text.as_bytes();
    let max_check = text_bytes.len().min(pattern_bytes.len());

    for len in (1..=max_check).rev() {
        let suffix = &text_bytes[text_bytes.len() - len..];
        let prefix = &pattern_bytes[..len];
        if suffix == prefix {
            return len;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_context() {
        let input = "Hello <memory-context>secret stuff</memory-context> world";
        let result = sanitize_context(input);
        assert_eq!(result, "Hello  world");
    }

    #[test]
    fn test_sanitize_context_multiline() {
        let input = "Before\n<memory-context>\nline1\nline2\n</memory-context>\nAfter";
        let result = sanitize_context(input);
        assert_eq!(result, "Before\n\nAfter");
    }

    #[test]
    fn test_sanitize_no_match() {
        let input = "No context blocks here";
        let result = sanitize_context(input);
        assert_eq!(result, "No context blocks here");
    }

    #[test]
    fn test_streaming_scrubber_no_blocks() {
        let mut scrubber = StreamingContextScrubber::new();
        assert_eq!(scrubber.process_chunk("hello"), Some("hello".into()));
        assert_eq!(scrubber.process_chunk(" world"), Some(" world".into()));
    }

    #[test]
    fn test_streaming_scrubber_complete_block() {
        let mut scrubber = StreamingContextScrubber::new();
        assert_eq!(
            scrubber.process_chunk("before<memory-context>secret</memory-context>after"),
            Some("beforeafter".into())
        );
    }

    #[test]
    fn test_streaming_scrubber_split_block() {
        let mut scrubber = StreamingContextScrubber::new();
        assert_eq!(
            scrubber.process_chunk("before<memory-context>sec"),
            Some("before".into())
        );
        assert_eq!(
            scrubber.process_chunk("ret</memory-context>after"),
            Some("after".into())
        );
    }

    #[test]
    fn test_streaming_scrubber_split_tag() {
        let mut scrubber = StreamingContextScrubber::new();
        // The opening tag is split across chunks
        assert_eq!(
            scrubber.process_chunk("before<memory-"),
            Some("before".into())
        );
        assert_eq!(
            scrubber.process_chunk("context>secret</memory-context>after"),
            Some("after".into())
        );
    }

    #[test]
    fn test_find_partial_match() {
        assert_eq!(find_partial_match("hello<memory-", "<memory-context>"), 8);
        assert_eq!(find_partial_match("no match", "<memory-context>"), 0);
        assert_eq!(find_partial_match("<memory-context", "<memory-context>"), 15);
    }

    #[test]
    fn test_streaming_flush() {
        let mut scrubber = StreamingContextScrubber::new();
        scrubber.process_chunk("hello<memory-context>sec");
        assert_eq!(scrubber.flush(), None); // buffer consumed by inside_block
    }
}
