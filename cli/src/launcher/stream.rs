//! Dev-server terminal output as shown on the PC. Yougori's markers become events, only harmless
//! terminal sequences pass (colours, line and screen clearing, cursor movement), and localhost
//! links are rewritten to the port that works on this PC.

pub struct Stream {
    marker: Vec<u8>,
    pending: Vec<u8>,
    escape: Vec<u8>,
    last: u8,
    /// Container app port → the PC's local port, when they differ.
    ports: Option<(u16, u16)>,
    colour: bool,
    c2: bool,
    link_tail: Vec<u8>,
    marker_limit: usize,
}

const ESC: u8 = 0x1b;

impl Stream {
    pub fn new(nonce: &str, ports: Option<(u16, u16)>, colour: bool) -> Self {
        Self {
            marker: format!("[yougori-{nonce}:").into_bytes(),
            pending: Vec::new(),
            escape: Vec::new(),
            last: b'\n',
            ports: ports.filter(|(a, b)| a != b),
            colour,
            c2: false,
            link_tail: Vec::new(),
            marker_limit: 512,
        }
    }

    pub fn protocol(mut self) -> Self { self.marker_limit = 16384; self }

    /// Returns bytes to print and the markers found, in order.
    pub fn feed(&mut self, bytes: &[u8]) -> (Vec<u8>, Vec<String>) {
        self.pending.extend_from_slice(bytes);
        let mut out = Vec::new();
        let mut events = Vec::new();
        loop {
            let Some(start) = find(&self.pending, &self.marker) else {
                let mut keep = partial_suffix(&self.pending, &self.marker);
                if keep > 0 {
                    let before = self.pending.len() - keep;
                    if self.pending[..before].ends_with(b"\r\n") {
                        keep += 2;
                    } else if self.pending[..before].ends_with(b"\n") {
                        keep += 1;
                    }
                }
                let ready: Vec<u8> = self.pending.drain(..self.pending.len() - keep).collect();
                self.display(&ready, &mut out);
                break;
            };
            let Some(length) = self.pending[start..].iter().position(|b| *b == b']') else {
                if self.pending.len() - start > self.marker_limit {
                    let ready: Vec<u8> = self.pending.drain(..start + 1).collect();
                    self.display(&ready, &mut out);
                    continue;
                }
                let ready: Vec<u8> = self.pending.drain(..start).collect();
                self.display(&ready, &mut out);
                break;
            };
            let end = start + length;
            events.push(
                String::from_utf8_lossy(&self.pending[start + self.marker.len()..end]).into_owned(),
            );
            // The marker was printed on its own line; do not leave an empty line behind.
            let mut before = start;
            let newline = if self.pending[..before].ends_with(b"\r\n") {
                2
            } else if self.pending[..before].ends_with(b"\n") {
                1
            } else {
                0
            };
            if newline > 0 {
                let previous = if before > newline {
                    self.pending[before - newline - 1]
                } else {
                    self.last
                };
                if previous == b'\n' {
                    before -= newline;
                }
            }
            let mut after = end + 1;
            if self.pending[after..].starts_with(b"\r\n") {
                after += 2;
            } else if self.pending[after..].starts_with(b"\n") {
                after += 1;
            }
            let ready = self.pending[..before].to_vec();
            self.display(&ready, &mut out);
            self.pending.drain(..after);
        }
        if let Some((from, to)) = self.ports {
            self.link_tail.extend(out);
            let keep = partial_link(&self.link_tail, from);
            let ready = self.link_tail.len() - keep;
            out = rewrite_ports(&self.link_tail[..ready], from, to);
            self.link_tail.drain(..ready);
        }
        (out, events)
    }

    fn display(&mut self, bytes: &[u8], out: &mut Vec<u8>) {
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if self.c2 {
                self.c2 = false;
                if (0x80..=0x9f).contains(&b) {
                    i += 1;
                    continue;
                }
                out.push(0xc2);
            }
            if !self.escape.is_empty() {
                self.escape.push(b);
                if let Some(keep) = finished(&self.escape) {
                    if keep && self.colour {
                        out.extend_from_slice(&self.escape);
                    }
                    self.escape.clear();
                } else if self.escape.len() > 64 {
                    if matches!(self.escape.get(1), Some(b']' | b'P' | b'_' | b'^')) {
                        // Keep discarding long OSC/DCS strings until their terminator.
                        self.escape.truncate(2);
                        if b == ESC {
                            self.escape.push(ESC);
                        }
                    } else {
                        self.escape.clear();
                    }
                }
            } else if b == ESC {
                self.escape.push(b);
            } else if b == 0xc2 {
                self.c2 = true;
            } else if b >= 0x20 && b != 0x7f || matches!(b, b'\n' | b'\r' | b'\t' | 0x08) {
                out.push(b);
                self.last = b;
            }
            i += 1;
        }
    }
}

/// Whether an escape sequence is complete: Some(true) to keep it, Some(false) to drop it.
fn finished(sequence: &[u8]) -> Option<bool> {
    match sequence.get(1) {
        None => None,
        Some(b'[') => {
            let last = *sequence.last().unwrap();
            if sequence.len() < 3 || !(0x40..=0x7e).contains(&last) {
                return None;
            }
            let params = &sequence[2..sequence.len() - 1];
            let plain = params.iter().all(|b| b.is_ascii_digit() || *b == b';');
            Some(
                (plain && b"mKJABCDEFGHfsu".contains(&last))
                    || params == b"?25" && matches!(last, b'h' | b'l'),
            )
        }
        // OSC (titles, hyperlinks, clipboard) and DCS strings are dropped whole.
        Some(b']' | b'P' | b'_' | b'^') => {
            (sequence.ends_with(&[0x07]) || sequence.ends_with(&[ESC, b'\\'])).then_some(false)
        }
        Some(b'7' | b'8') => Some(true),
        Some(_) => Some(false),
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Length of the longest end of `bytes` that could begin `needle`.
fn partial_suffix(bytes: &[u8], needle: &[u8]) -> usize {
    (1..needle.len().min(bytes.len() + 1))
        .rev()
        .find(|n| bytes.ends_with(&needle[..*n]))
        .unwrap_or(0)
}

// Hold only a suffix that could be a localhost link split across terminal reads.
fn partial_link(bytes: &[u8], port: u16) -> usize {
    let number = port.to_string();
    for start in bytes.len().saturating_sub(128)..bytes.len() {
        let tail = &bytes[start..];
        for host in [&b"localhost:"[..], b"127.0.0.1:", b"0.0.0.0:", b"[::1]:"] {
            if host.starts_with(tail) {
                return tail.len();
            }
            if !tail.starts_with(host) {
                continue;
            }
            let mut rest = &tail[host.len()..];
            while rest.starts_with(&[ESC]) {
                let Some(end) = rest.iter().position(|b| *b == b'm') else {
                    return tail.len();
                };
                rest = &rest[end + 1..];
            }
            if number.as_bytes().starts_with(rest) {
                return tail.len();
            }
        }
    }
    0
}

/// `localhost:FROM` (colours allowed between the colon and the number) becomes `localhost:TO`.
pub fn rewrite_ports(text: &[u8], from: u16, to: u16) -> Vec<u8> {
    let from = from.to_string().into_bytes();
    let to = to.to_string().into_bytes();
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    'outer: while i < text.len() {
        for host in [&b"localhost:"[..], b"127.0.0.1:", b"0.0.0.0:", b"[::1]:"] {
            if text[i..].starts_with(host) {
                let mut j = i + host.len();
                while text.get(j) == Some(&ESC) && text.get(j + 1) == Some(&b'[') {
                    match text[j..].iter().position(|b| *b == b'm') {
                        Some(end) => j += end + 1,
                        None => break,
                    }
                }
                if text[j..].starts_with(&from)
                    && !text.get(j + from.len()).is_some_and(u8::is_ascii_digit)
                {
                    out.extend_from_slice(if host == b"0.0.0.0:" {
                        b"localhost:"
                    } else {
                        host
                    });
                    out.extend_from_slice(&text[i + host.len()..j]);
                    out.extend_from_slice(&to);
                    i = j + from.len();
                    continue 'outer;
                }
            }
        }
        out.push(text[i]);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_payload_survives_split_reads_without_printing_file_contents() {
        let payload = "A".repeat(8192);
        let bytes = format!("\n[yougori-test:rpc:seq:{payload}]\n[yougori-test:end:seq]\n");
        let mut stream = Stream::new("test", None, false).protocol();
        let mut events = Vec::new();
        for chunk in bytes.as_bytes().chunks(133) {
            let (text, found) = stream.feed(chunk);
            assert!(!text.contains(&b'A'), "File contents must remain protocol data");
            events.extend(found);
        }
        assert_eq!(events, vec![format!("rpc:seq:{payload}"), "end:seq".into()]);
    }

    fn run(stream: &mut Stream, chunks: &[&[u8]]) -> (String, Vec<String>) {
        let mut text = Vec::new();
        let mut all = Vec::new();
        for chunk in chunks {
            let (out, events) = stream.feed(chunk);
            text.extend(out);
            all.extend(events);
        }
        (String::from_utf8(text).unwrap(), all)
    }

    #[test]
    fn markers_become_events_even_when_split_across_reads() {
        let mut stream = Stream::new("n1", None, true);
        let (text, events) = run(
            &mut stream,
            &[b"hello\r\n\r\n[you", b"gori-n1:ready:5173]\r\nworld\r\n"],
        );
        assert_eq!(text, "hello\r\nworld\r\n");
        assert_eq!(events, ["ready:5173"]);
        let (text, events) = run(&mut stream, &[b"spinner...\r\n[yougori-n1:exited:1]\r\n"]);
        assert_eq!(text, "spinner...\r\n");
        assert_eq!(events, ["exited:1"]);
        let (text, _) = run(&mut stream, &[b"[yougori-other:ready]\n"]);
        assert_eq!(
            text, "[yougori-other:ready]\n",
            "another nonce is ordinary output"
        );
    }

    #[test]
    fn only_harmless_terminal_sequences_pass() {
        let mut stream = Stream::new("n", None, true);
        let (text, _) = run(&mut stream, &[b"\x1b[32mgreen\x1b[0m \x1b[2J\x1b[H\x1b]0;title\x07\x1b]52;c;cGFzdGU=\x07\x1b[?1049h\x1b[6n\x1b[?25lok\x07\xc2\x9b31m"]);
        assert_eq!(text, "\x1b[32mgreen\x1b[0m \x1b[2J\x1b[H\x1b[?25lok31m");
        let mut plain = Stream::new("n", None, false);
        let (text, _) = run(&mut plain, &[b"\x1b[3", b"2mgreen\x1b[0m"]);
        assert_eq!(text, "green");
        let long = format!("\x1b]52;c;{}\x07safe", "x".repeat(1000));
        assert_eq!(run(&mut plain, &[long.as_bytes()]).0, "safe");
        assert_eq!(run(&mut plain, &[b"\xc2", b"\x9b31m"]).0, "31m");
    }

    #[test]
    fn local_links_use_the_port_open_on_this_pc() {
        let mut stream = Stream::new("n", Some((5173, 5174)), true);
        let (text, _) = run(&mut stream, &[b"  Local:   http://localhost:\x1b[1m5173\x1b[22m/\r\n  also 127.0.0.1:51730 and 0.0.0.0:5173\r\n"]);
        assert_eq!(
            run(&mut stream, &[b"http://local", b"host:5", b"173/\n"]).0,
            "http://localhost:5174/\n"
        );
        assert_eq!(text, "  Local:   http://localhost:\x1b[1m5174\x1b[22m/\r\n  also 127.0.0.1:51730 and localhost:5174\r\n");
    }
}
