//! CONCEPT: bounded ring buffer of child-process output lines (newest kept, oldest dropped).
//! WHY: the agent must keep rpc-server's stdout/stderr for diagnosis (R3.4) without unbounded memory: with
//!      `GGML_RPC_DEBUG` on (O-08, WP-6 6A) rpc-server logs per tensor and can print millions of lines.
//!      `dropped` counts what fell off, so a reader knows the tail is partial.
//! DEPENDS ON: nothing.
//! AFFECTS: supervisor.rs (writer), WP-6 6A relay telemetry and WP-7 status (readers, later).

use serde::Serialize;
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogLine {
    /// 0-based count of lines ever pushed: gaps between readers' snapshots are visible.
    pub seq: u64,
    pub stream: Stream,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct LogRing {
    cap: usize,
    lines: VecDeque<LogLine>,
    next_seq: u64,
    dropped: u64,
}

impl LogRing {
    /// `cap` is clamped to at least 1.
    pub fn new(cap: usize) -> Self {
        let cap = cap.max(1);
        Self {
            cap,
            lines: VecDeque::with_capacity(cap.min(1024)),
            next_seq: 0,
            dropped: 0,
        }
    }

    pub fn push(&mut self, stream: Stream, text: impl Into<String>) {
        if self.lines.len() == self.cap {
            self.lines.pop_front();
            self.dropped += 1;
        }
        self.lines.push_back(LogLine {
            seq: self.next_seq,
            stream,
            text: text.into(),
        });
        self.next_seq += 1;
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// Lines that fell off the front.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Oldest to newest copy of what is held now.
    pub fn snapshot(&self) -> Vec<LogLine> {
        self.lines.iter().cloned().collect()
    }

    /// The newest `n` lines, oldest first.
    pub fn tail(&self, n: usize) -> Vec<LogLine> {
        let skip = self.lines.len().saturating_sub(n);
        self.lines.iter().skip(skip).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_newest_and_counts_dropped() {
        let mut r = LogRing::new(3);
        for i in 0..5 {
            r.push(Stream::Stdout, format!("l{i}"));
        }
        assert_eq!(r.len(), 3);
        assert_eq!(r.dropped(), 2);
        let s: Vec<_> = r.snapshot().into_iter().map(|l| (l.seq, l.text)).collect();
        assert_eq!(
            s,
            [(2, "l2".into()), (3, "l3".into()), (4, "l4".into())] as [(u64, String); 3]
        );
    }

    #[test]
    fn tail_and_streams() {
        let mut r = LogRing::new(10);
        r.push(Stream::Stdout, "a");
        r.push(Stream::Stderr, "b");
        r.push(Stream::Stdout, "c");
        let t = r.tail(2);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].text, "b");
        assert_eq!(t[0].stream, Stream::Stderr);
        assert_eq!(r.tail(99).len(), 3);
        assert_eq!(r.dropped(), 0);
    }

    #[test]
    fn zero_capacity_is_clamped() {
        let mut r = LogRing::new(0);
        r.push(Stream::Stdout, "a");
        r.push(Stream::Stdout, "b");
        assert_eq!(r.capacity(), 1);
        assert_eq!(r.snapshot()[0].text, "b");
        assert!(!r.is_empty());
    }
}
