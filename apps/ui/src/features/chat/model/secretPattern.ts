// Whether what is being typed looks like a secret (a key, a token, a private key). The same shapes the engine masks when it
// exports a trace (crates/aiw-engine/src/traces.rs, `Redactor::new`): if the two lists drift, a secret the engine would mask
// goes unwarned. It is only a warning in the message box; nothing is blocked and nothing is stored here.
//
// One difference, on purpose: `sk-` must not be the tail of a word. The engine masks inside words too (harmless in a
// trace), but in the message box "disk-usage-analyzer-tool" or "task-force-ricerca" would raise a false alarm. It is written
// without a lookbehind (`(?<!…)`): WebKit has it only from Safari 16.4, and on an older macOS (the app runs on macOS 11) the
// literal is a SyntaxError when the file is parsed, which would take the whole chat block down with it.
//
// Not covered, as in the engine: tokens of providers that have no fixed prefix, and `hf_…` tokens of Hugging Face.

export const SECRET_PATTERNS: readonly RegExp[] = [
  /-----BEGIN [A-Z ]*PRIVATE KEY-----/,
  /(?:^|[^A-Za-z0-9])sk-[A-Za-z0-9_-]{16,}/,
  /gh[pousr]_[A-Za-z0-9]{20,}/,
  /github_pat_[A-Za-z0-9_]{20,}/,
  /\bAKIA[0-9A-Z]{16}\b/,
  /xox[bp]-[A-Za-z0-9-]{10,}/,
  /\bBearer\s+[A-Za-z0-9._~+/=-]{8,}/i,
];

/** The text holds something shaped like a secret. */
export const looksLikeSecret = (text: string): boolean => text !== "" && SECRET_PATTERNS.some((re) => re.test(text));
