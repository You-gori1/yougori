/** Pick the longest token-shaped string from copied Cloudflare instructions. */
export function cloudflareTokenInput(value: string): string {
  const candidates = value.match(/[A-Za-z0-9+/_=.-]{32,}/g)
  if (!candidates || !/\s/.test(value)) return value
  return candidates.reduce((longest, candidate) => candidate.length > longest.length ? candidate : longest)
}
