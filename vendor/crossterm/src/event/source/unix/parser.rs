//! Byte-level terminal reply demultiplexing shared by both Unix event sources.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use crate::event::{sys::unix::parse::parse_event, InternalEvent, TerminalResponse};

const ESCAPE_DELAY: Duration = Duration::from_millis(100);
const PARTIAL_REPLY_IDLE: Duration = Duration::from_secs(2);
const MAX_REPLY_BYTES: usize = 1024;

#[derive(Debug, Default)]
pub(super) struct Parser {
    buffer: Vec<u8>,
    candidate: Vec<u8>,
    internal_events: VecDeque<InternalEvent>,
    last_candidate_byte: Option<Instant>,
}

enum Reply {
    Pending,
    Complete(TerminalResponse),
    Tmux(Vec<u8>),
    Input,
}

impl Parser {
    pub(super) fn advance(&mut self, bytes: &[u8], more: bool) {
        for (idx, &byte) in bytes.iter().enumerate() {
            let more = idx + 1 < bytes.len() || more;
            // A paste is opaque user data, even if it contains protocol-looking text.
            if self.buffer.starts_with(b"\x1b[200~") {
                self.decode(byte, more);
                continue;
            }
            if self.candidate.is_empty() {
                if byte == 0x1b && self.buffer.is_empty() {
                    self.candidate.push(byte);
                    self.last_candidate_byte = Some(Instant::now());
                } else {
                    self.decode(byte, more);
                }
                continue;
            }
            self.candidate.push(byte);
            self.last_candidate_byte = Some(Instant::now());
            match classify(&self.candidate) {
                Reply::Pending if self.candidate.len() <= MAX_REPLY_BYTES => {}
                Reply::Complete(response) => {
                    // Keep the existing keyboard-enhancement query's DA1 contract.
                    if matches!(response, TerminalResponse::PrimaryDeviceAttributes(_)) {
                        self.internal_events
                            .push_back(InternalEvent::PrimaryDeviceAttributes);
                    }
                    self.internal_events
                        .push_back(InternalEvent::TerminalResponse(response));
                    self.candidate.clear();
                    self.last_candidate_byte = None;
                }
                Reply::Tmux(payload) => {
                    self.candidate.clear();
                    self.last_candidate_byte = None;
                    self.advance(&payload, more);
                }
                _ => self.replay(more),
            }
        }
    }

    fn decode(&mut self, byte: u8, more: bool) {
        self.buffer.push(byte);
        // Unknown CSI parameters must not reset the decoder before the final byte.
        // X10 mouse, Linux function keys, and bracketed paste have trailing payloads.
        let framed_csi = self.buffer.starts_with(b"\x1b[")
            && self.buffer.len() > 2
            && !self.buffer.starts_with(b"\x1b[M")
            && !self.buffer.starts_with(b"\x1b[[")
            && !self.buffer.starts_with(b"\x1b[200~");
        if framed_csi && !(0x40..=0x7e).contains(&byte) {
            return;
        }
        match parse_event(&self.buffer, more) {
            Ok(Some(event)) => {
                self.internal_events.push_back(event);
                self.buffer.clear();
                self.last_candidate_byte = None;
            }
            Ok(None) if !framed_csi => {}
            Ok(None) => {
                self.buffer.clear();
                self.last_candidate_byte = None;
            }
            Err(_) => {
                self.buffer.clear();
                self.last_candidate_byte = None;
            }
        }
    }

    fn replay(&mut self, more: bool) {
        let bytes = std::mem::take(&mut self.candidate);
        for (idx, &byte) in bytes.iter().enumerate() {
            self.decode(byte, idx + 1 < bytes.len() || more);
        }
    }

    /// Bound lone-Escape ambiguity without splitting a reply across read calls.
    pub(super) fn poll_timeout(&self, timeout: Option<Duration>) -> Option<Duration> {
        if let Some(delay) = self.candidate_timeout() {
            Some(timeout.map_or(delay, |timeout| timeout.min(delay)))
        } else {
            timeout
        }
    }

    fn candidate_timeout(&self) -> Option<Duration> {
        if self.ambiguous_prefix() {
            Some(ESCAPE_DELAY.saturating_sub(self.last_candidate_byte.unwrap().elapsed()))
        } else if !self.candidate.is_empty() {
            Some(PARTIAL_REPLY_IDLE.saturating_sub(self.last_candidate_byte.unwrap().elapsed()))
        } else {
            None
        }
    }

    fn ambiguous_prefix(&self) -> bool {
        // Alt-_ and Alt-P are user keys until the identifying reply prefix arrives.
        self.candidate == b"\x1b"
            || [b"\x1b_Gi=31;".as_slice(), b"\x1bPtmux;".as_slice()]
                .iter()
                .any(|prefix| {
                    self.candidate.len() < prefix.len()
                        && !self.candidate.is_empty()
                        && prefix.starts_with(&self.candidate)
                })
    }
}

impl Iterator for Parser {
    type Item = InternalEvent;

    fn next(&mut self) -> Option<Self::Item> {
        if self
            .candidate_timeout()
            .is_some_and(|delay| delay.is_zero())
        {
            self.replay(false);
        }
        self.internal_events.pop_front()
    }
}

fn classify(bytes: &[u8]) -> Reply {
    const KITTY: &[u8] = b"\x1b_Gi=31;";
    const TMUX: &[u8] = b"\x1bPtmux;";
    const DA: &[u8] = b"\x1b[?";
    const CELL: &[u8] = b"\x1b[6;";
    const STATUS: &[u8] = b"\x1b[0n";
    if [KITTY, TMUX, DA, CELL, STATUS]
        .iter()
        .any(|prefix| prefix.starts_with(bytes))
    {
        return if bytes == STATUS {
            Reply::Complete(TerminalResponse::Status)
        } else {
            Reply::Pending
        };
    }
    if let Some(body) = bytes.strip_prefix(KITTY) {
        if let Some(body) = body.strip_suffix(b"\x1b\\") {
            return Reply::Complete(TerminalResponse::Kitty(body == b"OK"));
        }
        return if body
            .iter()
            .all(|byte| byte.is_ascii_graphic() || *byte == b' ' || *byte == 0x1b)
        {
            Reply::Pending
        } else {
            Reply::Input
        };
    }
    if let Some(body) = bytes.strip_prefix(TMUX) {
        let mut payload = Vec::new();
        let mut idx = 0;
        while idx < body.len() {
            if body[idx] != 0x1b {
                payload.push(body[idx]);
                idx += 1;
            } else if idx + 1 == body.len() {
                return Reply::Pending;
            } else if body[idx + 1] == 0x1b {
                payload.push(0x1b);
                idx += 2;
            } else if body[idx + 1] == b'\\' && idx + 2 == body.len() {
                return Reply::Tmux(payload);
            } else {
                return Reply::Input;
            }
        }
        return Reply::Pending;
    }
    if let Some(body) = bytes.strip_prefix(DA) {
        if let Some(body) = body.strip_suffix(b"c") {
            if let Some(attributes) = numbers(body) {
                return Reply::Complete(TerminalResponse::PrimaryDeviceAttributes(attributes));
            }
        } else if numeric_prefix(body) {
            return Reply::Pending;
        }
    }
    if let Some(body) = bytes.strip_prefix(CELL) {
        if let Some(body) = body.strip_suffix(b"t") {
            if let Some(numbers) = numbers(body) {
                if let [height, width] = numbers[..] {
                    if width > 0 && height > 0 {
                        return Reply::Complete(TerminalResponse::CellSize(width, height));
                    }
                }
            }
        } else if numeric_prefix(body) {
            return Reply::Pending;
        }
    }
    Reply::Input
}

fn numeric_prefix(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .all(|byte| byte.is_ascii_digit() || *byte == b';')
}

fn numbers(bytes: &[u8]) -> Option<Vec<u16>> {
    std::str::from_utf8(bytes)
        .ok()?
        .split(';')
        .map(|part| part.parse().ok())
        .collect()
}

#[cfg(all(test, feature = "bracketed-paste"))]
mod tests {
    use super::*;
    use crate::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };

    #[test]
    fn terminal_csi_frames_do_not_leak_as_text() {
        let wire = b"\x1b[>0;276;0c\x1b[=1;1u\x1b[?2026;2$y\x1b[<35;32;44M\x1b[99;5u\x1b[A\x1b[[A\x1b[M !!\x1b[200~[>literal\x1b[201~ok";
        let expected = vec![
            InternalEvent::Event(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Moved,
                column: 31,
                row: 43,
                modifiers: KeyModifiers::NONE,
            })),
            InternalEvent::Event(Event::Key(KeyEvent::new(
                KeyCode::Char('c'),
                KeyModifiers::CONTROL,
            ))),
            InternalEvent::Event(Event::Key(KeyCode::Up.into())),
            InternalEvent::Event(Event::Key(KeyCode::F(1).into())),
            InternalEvent::Event(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 0,
                row: 0,
                modifiers: KeyModifiers::NONE,
            })),
            InternalEvent::Event(Event::Paste("[>literal".into())),
            InternalEvent::Event(Event::Key(KeyCode::Char('o').into())),
            InternalEvent::Event(Event::Key(KeyCode::Char('k').into())),
        ];
        for split in 0..=wire.len() {
            let mut parser = Parser::default();
            parser.advance(&wire[..split], false);
            let mut events: Vec<_> = parser.by_ref().collect();
            parser.advance(&wire[split..], false);
            events.extend(parser);
            assert_eq!(events, expected, "split {split}");
        }
        let mut parser = Parser::default();
        let mut events = Vec::new();
        for byte in wire {
            parser.advance(&[*byte], false);
            events.extend(parser.by_ref());
        }
        assert_eq!(events, expected, "one byte per read");
    }

    // Reply bytes from the affected terminal, before crossterm decoded them as keys.
    const RECORDED: &[u8] = b"\x1b_Gi=31;OK\x1b\\\x1b[?62;4c\x1b[6;16;8t\x1b[0n";

    #[test]
    fn terminal_probe_replies_preserve_input_across_reads() {
        let expected_replies = vec![
            TerminalResponse::Kitty(true),
            TerminalResponse::PrimaryDeviceAttributes(vec![62, 4]),
            TerminalResponse::CellSize(8, 16),
            TerminalResponse::Status,
        ];
        let mut tmux = b"\x1bPtmux;".to_vec();
        for &byte in RECORDED {
            tmux.push(byte);
            if byte == 0x1b {
                tmux.push(byte);
            }
        }
        tmux.extend_from_slice(b"\x1b\\");
        // Literal text resembling the leaked suffix must not be removed.
        let text = "helloé_Gi=31;OK\\?62;4c6;16;8t0n";
        for replies in [RECORDED, tmux.as_slice()] {
            let mut wire = text.as_bytes().to_vec();
            wire.extend_from_slice(replies);
            wire.extend_from_slice(b"\x1b[A\x1b[200~");
            wire.extend_from_slice(RECORDED);
            wire.extend_from_slice(b"\x1b[201~!");
            let mut expected_input: Vec<_> = text
                .chars()
                .map(|c| {
                    Event::Key(KeyEvent::new(
                        KeyCode::Char(c),
                        if c.is_uppercase() {
                            KeyModifiers::SHIFT
                        } else {
                            KeyModifiers::NONE
                        },
                    ))
                })
                .collect();
            expected_input.push(Event::Key(KeyCode::Up.into()));
            expected_input.push(Event::Paste(String::from_utf8(RECORDED.to_vec()).unwrap()));
            expected_input.push(Event::Key(KeyCode::Char('!').into()));
            // Every two-read split includes splits inside ESC, ST, UTF-8, and tmux escaping.
            for split in 0..=wire.len() {
                let mut parser = Parser::default();
                parser.advance(&wire[..split], false);
                let mut events: Vec<_> = parser.by_ref().collect();
                parser.advance(&wire[split..], false);
                events.extend(parser.by_ref());
                let input: Vec<_> = events
                    .iter()
                    .filter_map(|event| match event {
                        InternalEvent::Event(event) => Some(event.clone()),
                        _ => None,
                    })
                    .collect();
                let parsed: Vec<_> = events
                    .iter()
                    .filter_map(|event| match event {
                        InternalEvent::TerminalResponse(reply) => Some(reply.clone()),
                        _ => None,
                    })
                    .collect();
                assert_eq!(input, expected_input, "split {split}");
                assert_eq!(parsed, expected_replies, "split {split}");
            }
        }

        // A status response can arrive first, and later batches still carry capabilities.
        let mut parser = Parser::default();
        parser.advance(b"\x1b[0n", false);
        assert_eq!(
            parser.next(),
            Some(InternalEvent::TerminalResponse(TerminalResponse::Status))
        );
        for byte in b"typed\x1b_Gi=31;OK\x1b\\\x1b[?62;4c\x1b[6;16;8t" {
            parser.advance(&[*byte], false);
        }
        let events: Vec<_> = parser.collect();
        let text: String = events
            .iter()
            .filter_map(|event| match event {
                InternalEvent::Event(Event::Key(key)) => match key.code {
                    KeyCode::Char(c) => Some(c),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(text, "typed");
        assert!(events.contains(&InternalEvent::TerminalResponse(
            TerminalResponse::CellSize(8, 16)
        )));

        // Real terminal reads can separate ESC from its reply by more than 25ms.
        let mut parser = Parser::default();
        for byte in b"\x1b_Gi=31;" {
            parser.advance(&[*byte], false);
            std::thread::sleep(Duration::from_millis(40));
            assert_eq!(parser.next(), None, "a fragmented reply is not user input");
        }
        parser.advance(b"OK\x1b\\", false);
        assert_eq!(
            parser.next(),
            Some(InternalEvent::TerminalResponse(TerminalResponse::Kitty(
                true
            )))
        );
    }
}
