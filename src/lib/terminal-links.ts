export const MAX_TERMINAL_LINKS = 8
const MAX_URL_LENGTH = 2048

export function normalizeTerminalLink(value: string): string | null {
  if (value.length > MAX_URL_LENGTH) return null
  try {
    const url = new URL(/^www\./i.test(value) ? `https://${value}` : value)
    if (!["http:", "https:"].includes(url.protocol) || !url.hostname || url.username || url.password) return null
    return url.href.length <= MAX_URL_LENGTH ? url.href : null
  } catch { return null }
}

export function extractTerminalLinks(text: string): string[] {
  const links: string[] = []
  // Input from xterm is already ANSI-decoded. Bound other console callers too.
  // Serial consoles may still contain ANSI styling (xterm callers do not).
  // eslint-disable-next-line no-control-regex -- strip terminal CSI control sequences
  const plain = text.slice(-65536).replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, "")
  // eslint-disable-next-line no-control-regex -- never include terminal control bytes in a URL
  for (const match of plain.matchAll(/(?:https?:\/\/|www\.)[^\s<>"'`\x00-\x1f]+/gi)) {
    let candidate = match[0].replace(/[.,;!]+$/, "")
    // Remove surrounding prose punctuation without damaging balanced URL paths.
    for (const [open, close] of [["(", ")"], ["[", "]"], ["{", "}"]] as const) {
      while (candidate.endsWith(close) && candidate.split(close).length > candidate.split(open).length) candidate = candidate.slice(0, -1)
    }
    const url = normalizeTerminalLink(candidate)
    if (url && !links.includes(url)) links.push(url)
  }
  return links.slice(-MAX_TERMINAL_LINKS)
}

export function mergeTerminalLinks(previous: string[], incoming: string[]): string[] {
  const next = [...previous]
  for (const url of incoming) if (!next.includes(url)) next.push(url)
  const bounded = next.slice(-MAX_TERMINAL_LINKS)
  return previous.length === bounded.length && previous.every((url, index) => url === bounded[index]) ? previous : bounded
}

export function isLocalTerminalLink(value: string): boolean {
  const host = new URL(value).hostname.toLowerCase()
  return host === "localhost" || host.endsWith(".localhost") || host === "0.0.0.0" || host.startsWith("127.") || host === "[::1]" || host === "[::]"
}
