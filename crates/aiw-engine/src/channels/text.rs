//! Pure text helpers of the channels: splitting to a platform's size limit, a safe downgrade of
//! Markdown to plain text, and the reading of a «sì» / «no» answer.

use regex::Regex;
use std::sync::OnceLock;

/// Splits `text` into pieces of at most `limit` characters, cutting at a blank line, a line end, a
/// sentence end or a space (in that order of preference) and only in the middle of a word when there
/// is nothing else. Empty pieces are never returned; the pieces joined give back the text (apart from
/// the whitespace at the cuts).
pub fn split_message(text: &str, limit: usize) -> Vec<String> {
    let limit = limit.max(10);
    let mut out = Vec::new();
    let mut rest: Vec<char> = text.trim().chars().collect();
    while !rest.is_empty() {
        if rest.len() <= limit {
            out.push(rest.iter().collect::<String>().trim().to_string());
            break;
        }
        let window = &rest[..limit];
        let find = |pred: &dyn Fn(usize) -> bool| (limit / 3..limit).rev().find(|&i| pred(i));
        let cut = find(&|i| window[i - 1] == '\n' && window.get(i - 2) == Some(&'\n'))
            .or_else(|| find(&|i| window[i - 1] == '\n'))
            .or_else(|| find(&|i| matches!(window[i - 1], '.' | '!' | '?') && window.get(i) == Some(&' ')))
            .or_else(|| find(&|i| window[i - 1] == ' '))
            .unwrap_or(limit);
        let piece: String = rest[..cut].iter().collect();
        let piece = piece.trim();
        if !piece.is_empty() {
            out.push(piece.to_string());
        }
        rest.drain(..cut);
        while rest.first().is_some_and(|c| c.is_whitespace()) {
            rest.remove(0);
        }
    }
    out
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("static regex"))
}

/// Markdown to plain text without losing information: headings lose their `#`, bold and italic their
/// markers, a link `[testo](url)` becomes `testo (url)`, a code block keeps its lines, a bullet becomes `•`.
/// Platforms that would try to interpret the Markdown (and fail on the first unbalanced `_`) get text instead.
pub fn plain_text(md: &str) -> String {
    static FENCE: OnceLock<Regex> = OnceLock::new();
    static HEAD: OnceLock<Regex> = OnceLock::new();
    static LINK: OnceLock<Regex> = OnceLock::new();
    static BOLD: OnceLock<Regex> = OnceLock::new();
    static ITALIC: OnceLock<Regex> = OnceLock::new();
    static CODE: OnceLock<Regex> = OnceLock::new();
    static BULLET: OnceLock<Regex> = OnceLock::new();
    static RULE: OnceLock<Regex> = OnceLock::new();
    let s = re(&FENCE, r"(?m)^\s*```[a-zA-Z0-9_+-]*\s*$\n?").replace_all(md, "");
    let s = re(&HEAD, r"(?m)^\s{0,3}#{1,6}\s+").replace_all(&s, "");
    let s = re(&LINK, r"!?\[([^\]]*)\]\(([^)\s]+)[^)]*\)").replace_all(&s, |c: &regex::Captures| {
        let (t, u) = (&c[1], &c[2]);
        if t.is_empty() || t == u {
            u.to_string()
        } else {
            format!("{t} ({u})")
        }
    });
    let s = re(&BOLD, r"(\*\*|__)(.+?)(\*\*|__)").replace_all(&s, "$2");
    let s = re(&ITALIC, r"(^|[\s(])[*_]([^*_\n]+)[*_]($|[\s).,;:!?])").replace_all(&s, "$1$2$3");
    let s = re(&CODE, r"`([^`\n]+)`").replace_all(&s, "$1");
    let s = re(&BULLET, r"(?m)^(\s*)[-*+]\s+").replace_all(&s, "$1• ");
    let s = re(&RULE, r"(?m)^\s*([-*_]\s*){3,}$").replace_all(&s, "");
    s.trim().to_string()
}

/// Text that Slack shows as it is: `&`, `<` and `>` are the only characters it interprets, and `<!channel>`
/// or `<@U1>` coming out of a model must never become a mention.
pub fn slack_escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// «sì» → `Some(true)`, «no» → `Some(false)`, anything else → `None`. Only an answer that is nothing but
/// the word counts: «sì, ma prima dimmi…» is a new message, not a yes.
pub fn parse_answer(text: &str) -> Option<bool> {
    let t: String = text.trim().trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
    match t.as_str() {
        "sì" | "si" | "s" | "ok" | "yes" | "y" | "va bene" | "certo" | "conferma" | "confermo" | "approvo" | "👍" => Some(true),
        "no" | "n" | "annulla" | "nope" | "rifiuto" | "nega" | "stop" | "👎" => Some(false),
        _ => None,
    }
}

/// A short, one-line excerpt for the activity log.
pub fn excerpt(text: &str, max: usize) -> String {
    let one: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= max {
        one
    } else {
        format!("{}…", one.chars().take(max).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_one_piece() {
        assert_eq!(split_message("ciao", 4000), vec!["ciao"]);
        assert!(split_message("   ", 4000).is_empty());
    }

    #[test]
    fn long_text_is_cut_at_natural_places_and_nothing_is_lost() {
        let para = "Questa è una frase abbastanza lunga da riempire la riga. ".repeat(6);
        let text = format!("{para}\n\n{para}\n\n{para}");
        let parts = split_message(&text, 400);
        assert!(parts.len() >= 3);
        assert!(parts.iter().all(|p| p.chars().count() <= 400 && !p.is_empty()), "{parts:?}");
        let rebuilt: String = parts.join(" ");
        assert_eq!(rebuilt.split_whitespace().collect::<Vec<_>>(), text.split_whitespace().collect::<Vec<_>>());
        // The cut prefers the blank line to the middle of a sentence.
        assert!(parts[0].ends_with('.'), "{}", parts[0]);
    }

    #[test]
    fn a_word_longer_than_the_limit_is_cut_hard_and_multibyte_is_safe() {
        let parts = split_message(&"à".repeat(25), 10);
        assert_eq!(parts.iter().map(|p| p.chars().count()).collect::<Vec<_>>(), vec![10, 10, 5]);
        assert_eq!(split_message(&"😀".repeat(9), 4).concat(), "😀".repeat(9));
    }

    #[test]
    fn markdown_becomes_plain_text() {
        let md = "# Titolo\n\nUn **grassetto**, un _corsivo_ e `codice`.\n\n- uno\n- due\n\n[sito](https://x.it) e https://y.it\n\n```rust\nlet a = 1;\n```\n";
        let p = plain_text(md);
        assert!(p.starts_with("Titolo\n"), "{p}");
        assert!(p.contains("Un grassetto, un corsivo e codice."), "{p}");
        assert!(p.contains("• uno\n• due"), "{p}");
        assert!(p.contains("sito (https://x.it) e https://y.it"), "{p}");
        assert!(p.contains("let a = 1;") && !p.contains("```"), "{p}");
        // Unbalanced markers are left alone, not eaten.
        assert_eq!(plain_text("a_b_c e 2*3"), "a_b_c e 2*3");
    }

    #[test]
    fn slack_mentions_are_neutralised() {
        assert_eq!(slack_escape("<!channel> <@U1> a&b"), "&lt;!channel&gt; &lt;@U1&gt; a&amp;b");
    }

    #[test]
    fn answers_are_read_strictly() {
        for y in ["sì", "Sì!", " si ", "OK", "yes", "va bene"] {
            assert_eq!(parse_answer(y), Some(true), "{y}");
        }
        for n in ["no", "No.", "annulla", "n"] {
            assert_eq!(parse_answer(n), Some(false), "{n}");
        }
        for other in ["sì, ma prima dimmi cosa fa", "non lo so", "", "forse", "no grazie, fai altro"] {
            assert_eq!(parse_answer(other), None, "{other}");
        }
    }
}
