// Secrets of MCP server configurations. The backend (crates/aiw-engine/src/secrets.rs) never returns the value of a
// header or an environment variable whose name looks like a credential: it returns `SECRET_MASK`, and receiving
// the mask back on save means "keep what is stored".

export { SECRET_MASK } from "../api/types";

/** Name fragments (lower case) of the headers and variables whose value is a secret: same list as the backend. */
const SECRET_WORDS = ["auth", "cookie", "token", "key", "secret", "password", "passwd", "credential", "signature"];

/**
 * Does the header or variable called `name` carry a secret (`Authorization`, `Cookie`, `X-Api-Key`,
 * `GITHUB_TOKEN`, `DB_PASSWORD`…)? Substring match, whatever the case, like `is_secret_name`.
 */
export function isSecretName(name: string): boolean {
  const n = name.toLowerCase();
  return SECRET_WORDS.some((w) => n.includes(w));
}
