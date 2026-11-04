/** `Revisore di codice` → `revisore-di-codice`: lowercase, no accents, dashes between words. */
export function slugify(name: string, fallback = "elemento"): string {
  const s = name
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 40)
    .replace(/-+$/g, "");
  return s || fallback;
}

/** `base`, or `base-2`, `base-3`… when it is taken or reserved (the URL segment `new` is always reserved). */
export function uniqueSlug(base: string, taken: readonly string[], reserved: readonly string[] = ["new"]): string {
  const used = new Set([...taken, ...reserved]);
  if (!used.has(base)) return base;
  for (let n = 2; ; n++) if (!used.has(`${base}-${n}`)) return `${base}-${n}`;
}
