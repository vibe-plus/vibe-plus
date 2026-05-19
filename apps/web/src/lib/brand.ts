/** Product name in prose (CLI, descriptions, alt text). */
export const BRAND_NAME = "Vibe Plus";

/** Plain wordmark for `document.title`, meta tags, and non-HTML contexts. */
export const BRAND_WORDMARK_PLAIN = "Vibe+";

export function brandPageTitle(page: string): string {
  return `${page} · ${BRAND_WORDMARK_PLAIN}`;
}

export function brandHomeTitle(tagline: string): string {
  return `${BRAND_WORDMARK_PLAIN} · ${tagline}`;
}
