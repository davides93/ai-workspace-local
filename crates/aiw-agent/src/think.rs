//! Splits inline `<think>…</think>` blocks (Qwen3, DeepSeek-R1 style) out of the text stream.

/// Generic streaming splitter for `<open>…</close>` regions (thinking blocks, text tool calls).
pub struct TagSplitter {
    open: &'static str,
    close: &'static str,
    inside: bool,
    buf: String,
}

impl TagSplitter {
    pub fn new(open: &'static str, close: &'static str) -> Self {
        Self { open, close, inside: false, buf: String::new() }
    }

    /// Returns `(outside_text, inside_text)` pieces in order.
    pub fn push(&mut self, chunk: &str) -> Vec<(bool, String)> {
        self.buf.push_str(chunk);
        let mut out = Vec::new();
        loop {
            let tag = if self.inside { self.close } else { self.open };
            if let Some(i) = self.buf.find(tag) {
                let before: String = self.buf.drain(..i).collect();
                self.buf.drain(..tag.len());
                if !before.is_empty() {
                    out.push((self.inside, before));
                }
                self.inside = !self.inside;
                continue;
            }
            let keep = (1..tag.len()).rev().find(|n| self.buf.ends_with(&tag[..*n])).unwrap_or(0);
            let mut cut = self.buf.len() - keep;
            while !self.buf.is_char_boundary(cut) {
                cut -= 1;
            }
            if cut > 0 {
                let s: String = self.buf.drain(..cut).collect();
                out.push((self.inside, s));
            }
            break;
        }
        out
    }

    pub fn finish(&mut self) -> Vec<(bool, String)> {
        let s: String = self.buf.drain(..).collect();
        if s.is_empty() {
            vec![]
        } else {
            vec![(self.inside, s)]
        }
    }
}

#[derive(Default)]
pub struct ThinkSplitter {
    in_think: bool,
    buf: String,
}

#[derive(Debug, PartialEq)]
pub enum Piece {
    Text(String),
    Reasoning(String),
}

impl ThinkSplitter {
    pub fn push(&mut self, chunk: &str) -> Vec<Piece> {
        self.buf.push_str(chunk);
        let mut out = Vec::new();
        loop {
            let tag = if self.in_think { "</think>" } else { "<think>" };
            if let Some(i) = self.buf.find(tag) {
                let before: String = self.buf.drain(..i).collect();
                self.buf.drain(..tag.len());
                if !before.is_empty() {
                    out.push(if self.in_think { Piece::Reasoning(before) } else { Piece::Text(before) });
                }
                self.in_think = !self.in_think;
                continue;
            }
            // Keep a possible partial tag at the end for the next chunk.
            let keep = (1..tag.len()).rev().find(|n| self.buf.ends_with(&tag[..*n])).unwrap_or(0);
            let emit_len = self.buf.len() - keep;
            if emit_len > 0 {
                let mut cut = emit_len;
                while !self.buf.is_char_boundary(cut) {
                    cut -= 1;
                }
                let s: String = self.buf.drain(..cut).collect();
                if !s.is_empty() {
                    out.push(if self.in_think { Piece::Reasoning(s) } else { Piece::Text(s) });
                }
            }
            break;
        }
        out
    }

    pub fn finish(&mut self) -> Vec<Piece> {
        let s: String = self.buf.drain(..).collect();
        if s.is_empty() {
            vec![]
        } else if self.in_think {
            vec![Piece::Reasoning(s)]
        } else {
            vec![Piece::Text(s)]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hides_text_tool_calls_while_streaming() {
        let mut t = TagSplitter::new("<tool_call>", "</tool_call>");
        let mut shown = String::new();
        let mut hidden = String::new();
        for c in ["Controllo. <tool", "_call>{\"name\": \"x\"}</tool_", "call> fatto"] {
            for (inside, s) in t.push(c) {
                if inside {
                    hidden.push_str(&s)
                } else {
                    shown.push_str(&s)
                }
            }
        }
        for (inside, s) in t.finish() {
            if inside {
                hidden.push_str(&s)
            } else {
                shown.push_str(&s)
            }
        }
        assert_eq!(shown, "Controllo.  fatto");
        assert_eq!(hidden, "{\"name\": \"x\"}");
    }

    #[test]
    fn splits_across_chunks() {
        let mut s = ThinkSplitter::default();
        let mut all = Vec::new();
        for c in ["<thi", "nk>devo ", "pensare</th", "ink>\n\nRisposta", " finale"] {
            all.extend(s.push(c));
        }
        all.extend(s.finish());
        let reasoning: String = all.iter().filter_map(|p| if let Piece::Reasoning(r) = p { Some(r.as_str()) } else { None }).collect();
        let text: String = all.iter().filter_map(|p| if let Piece::Text(t) = p { Some(t.as_str()) } else { None }).collect();
        assert_eq!(reasoning, "devo pensare");
        assert_eq!(text, "\n\nRisposta finale");
    }
}
