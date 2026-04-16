// Cyrillic → Latin map (Russian + Ukrainian common letters).
// Lowercase only; callers normalize case before lookup.
const CYRILLIC_MAP: Record<string, string> = {
  а: "a", б: "b", в: "v", г: "g", д: "d", е: "e", ё: "yo", ж: "zh", з: "z",
  и: "i", й: "y", к: "k", л: "l", м: "m", н: "n", о: "o", п: "p", р: "r",
  с: "s", т: "t", у: "u", ф: "f", х: "h", ц: "ts", ч: "ch", ш: "sh", щ: "shch",
  ъ: "", ы: "y", ь: "", э: "e", ю: "yu", я: "ya",
  // Ukrainian / Belarusian extras
  і: "i", ї: "yi", є: "ye", ґ: "g", ў: "w",
};

/**
 * Best-effort transliteration to ASCII for use in git branch names.
 * - Strips combining diacritics (café → cafe)
 * - Maps Cyrillic letters to Latin equivalents
 * - Unmapped non-ASCII characters are dropped
 *
 * Not a general i18n solution — scripts like CJK, Arabic, Hebrew, Thai
 * collapse to empty. Callers must provide a fallback via `slugify`.
 */
export function transliterate(input: string): string {
  const lowered = input.toLowerCase();
  // Map Cyrillic FIRST — characters like ё, ї, й are precomposed and
  // NFD would decompose them into base + combining marks, collapsing the
  // distinction (ё → е + diaeresis → е after strip). After Cyrillic
  // mapping we're left with Latin (possibly accented) + unmapped scripts.
  let mapped = "";
  for (const ch of lowered) {
    const v = CYRILLIC_MAP[ch];
    mapped += v !== undefined ? v : ch;
  }
  // Now NFD-decompose and strip combining marks to normalize accented Latin
  // (café → cafe, über → uber). Harmless for already-ASCII output.
  return mapped.normalize("NFD").replace(/[\u0300-\u036f]/g, "");
}

/**
 * Produce a git-ref-safe slug. Empty input or input that transliterates to
 * nothing (e.g. pure emoji/CJK) returns an empty string — callers should
 * substitute a fallback like "task".
 */
export function slugify(input: string): string {
  return transliterate(input)
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}
