//! Pure helpers for the embedded terminal (#010): seeding the session prompt
//! from a task, splitting the configured run-command into argv, and the
//! scrollback ring buffer that lets a session be detached and reattached (#021).
//! The actual PTY + WebSocket plumbing lives in `web_terminal` (excluded from
//! coverage as untestable async/process glue); everything here is deterministic
//! and tested.

use std::collections::VecDeque;

use crate::taskmd::Task;

/// A byte ring buffer holding the most recent terminal output so a client that
/// (re)attaches to a running session can replay what it missed. Capped by byte
/// count; when full, the oldest bytes are dropped. Splitting an escape sequence
/// or a UTF-8 codepoint at the drop boundary is acceptable for a replay buffer:
/// terminals resync on the next write, and the live stream is always intact.
pub struct Scrollback {
    buf: VecDeque<u8>,
    cap: usize,
}

impl Scrollback {
    /// Create an empty buffer holding at most `cap` bytes (`cap` of 0 disables
    /// retention: nothing is ever kept).
    pub fn new(cap: usize) -> Self {
        Scrollback {
            buf: VecDeque::new(),
            cap,
        }
    }

    /// Append output, dropping oldest bytes to stay within the cap. If a single
    /// write exceeds the cap, only its trailing `cap` bytes are kept.
    pub fn push(&mut self, bytes: &[u8]) {
        if self.cap == 0 {
            return;
        }
        // Keep only the tail when a lone write is larger than the whole buffer.
        let tail = if bytes.len() > self.cap {
            &bytes[bytes.len() - self.cap..]
        } else {
            bytes
        };
        self.buf.extend(tail.iter().copied());
        while self.buf.len() > self.cap {
            self.buf.pop_front();
        }
    }

    /// A copy of the retained bytes, oldest first.
    pub fn snapshot(&self) -> Vec<u8> {
        self.buf.iter().copied().collect()
    }

    /// Retained byte count.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Whether anything is retained.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}

/// The initial prompt seeded into a run session from a task: id + title, then
/// the body when present. Not auto-submitted by the caller — the human reviews
/// it and hits enter (see `web_terminal`).
pub fn seed_prompt(task: &Task) -> String {
    let mut prompt = format!("Work on task {}: {}", task.id(), task.title());
    let body = task.body.trim();
    if !body.is_empty() {
        prompt.push_str("\n\n");
        prompt.push_str(body);
    }
    prompt
}

/// Split a command string into argv with minimal shell-like quoting: whitespace
/// separates tokens; single or double quotes group (and are stripped). A
/// backslash inside is kept literally (no escape processing). Good enough for
/// `claude`, `claude --flag`, or `"my tool" arg`. Empty input yields no argv.
pub fn parse_command(cmd: &str) -> Vec<String> {
    let mut argv = Vec::new();
    let mut cur = String::new();
    let mut in_token = false;
    let mut quote: Option<char> = None;
    for ch in cmd.chars() {
        match quote {
            Some(q) => {
                if ch == q {
                    quote = None;
                } else {
                    cur.push(ch);
                }
            }
            None => {
                if ch == '\'' || ch == '"' {
                    quote = Some(ch);
                    in_token = true;
                } else if ch.is_whitespace() {
                    if in_token {
                        argv.push(std::mem::take(&mut cur));
                        in_token = false;
                    }
                } else {
                    cur.push(ch);
                    in_token = true;
                }
            }
        }
    }
    if in_token {
        argv.push(cur);
    }
    argv
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskmd::Task;

    fn task(yaml: &str, body: &str) -> Task {
        Task::parse_required(&format!("---\n{yaml}\n---\n\n{body}\n")).unwrap()
    }

    #[test]
    fn seed_prompt_includes_id_title_and_body() {
        let t = task(
            "id: \"009\"\ntitle: Build the thing",
            "# Build the thing\n\nDo X then Y.",
        );
        let p = seed_prompt(&t);
        assert!(p.starts_with("Work on task 009: Build the thing"));
        assert!(p.contains("Do X then Y."));
    }

    #[test]
    fn seed_prompt_without_body_is_just_the_header() {
        // A task whose body is only the heading (trimmed to empty after the
        // heading is stripped) still yields a clean one-line prompt.
        let t = task("id: \"001\"\ntitle: Quick", "");
        let p = seed_prompt(&t);
        assert_eq!(p, "Work on task 001: Quick");
        assert!(!p.contains("\n"));
    }

    #[test]
    fn parse_command_splits_and_quotes() {
        assert_eq!(parse_command("claude"), vec!["claude"]);
        assert_eq!(parse_command("claude --print"), vec!["claude", "--print"]);
        assert_eq!(
            parse_command("  claude   --flag  value "),
            vec!["claude", "--flag", "value"]
        );
        assert_eq!(
            parse_command("\"my tool\" 'one arg' plain"),
            vec!["my tool", "one arg", "plain"]
        );
        // A quote immediately opening a token, and an empty quoted arg.
        assert_eq!(parse_command("cmd \"\""), vec!["cmd", ""]);
        assert!(parse_command("").is_empty());
        assert!(parse_command("   ").is_empty());
    }

    #[test]
    fn scrollback_retains_in_order_until_full() {
        let mut sb = Scrollback::new(8);
        assert!(sb.is_empty());
        sb.push(b"abc");
        sb.push(b"de");
        assert_eq!(sb.len(), 5);
        assert_eq!(sb.snapshot(), b"abcde");
        // Overflow drops from the front, keeping the most recent bytes.
        sb.push(b"fghij");
        assert_eq!(sb.len(), 8);
        assert_eq!(sb.snapshot(), b"cdefghij");
    }

    #[test]
    fn scrollback_keeps_tail_of_oversized_write() {
        let mut sb = Scrollback::new(4);
        sb.push(b"0123456789");
        assert_eq!(sb.snapshot(), b"6789");
    }

    #[test]
    fn scrollback_zero_cap_retains_nothing() {
        let mut sb = Scrollback::new(0);
        sb.push(b"anything");
        assert!(sb.is_empty());
        assert_eq!(sb.len(), 0);
        assert_eq!(sb.snapshot(), b"");
    }
}
