// Muted accents: blue, teal, violet, sage, ochre, rose, steel, plum.
const palette = ["#718fb5", "#589d96", "#9684b5", "#829b70", "#b49360", "#b47e89", "#6f9caa", "#a283a4", "#9c9670", "#729985", "#938fae", "#ad8873"]

/** Deterministic across graph reordering; resolve palette collisions for distinct nodes. */
export function graphColors(ids: string[]): Record<string, string> {
  const colors: Record<string, string> = {}
  const used = new Set<number>()
  for (const id of [...new Set(ids)].sort()) {
    let hash = 2166136261
    for (const char of id) hash = Math.imul(hash ^ char.charCodeAt(0), 16777619) >>> 0
    let slot = hash % palette.length
    if (used.size < palette.length) {
      while (used.has(slot)) slot = (slot + 1) % palette.length
    } else {
      slot = used.size
    }
    used.add(slot)
    colors[id] = palette[slot] ?? `hsl(${(slot * 137.508 % 360).toFixed(2)} 28% 57%)`
  }
  return colors
}
