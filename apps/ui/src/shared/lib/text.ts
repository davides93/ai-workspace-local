/** Lowercase and strip accents so "attivita" finds "Attività" (search boxes and filters). */
export function normalizeText(s: string): string {
  return s.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();
}

/** Every word of `query` (accent- and case-insensitive) appears in `haystack`. An empty query matches. */
export function matchesQuery(haystack: string, query: string): boolean {
  const words = normalizeText(query).split(/\s+/).filter(Boolean);
  if (words.length === 0) return true;
  const hay = normalizeText(haystack);
  return words.every((w) => hay.includes(w));
}
