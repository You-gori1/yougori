export interface TourRect { left: number; top: number; width: number; height: number }
const clamp = (value: number, low: number, high: number) => Math.min(Math.max(low, high), Math.max(low, value))
export function clipTourRect(rect: TourRect, width: number, height: number, padding = 6): TourRect {
  const left = clamp(rect.left - padding, 4, width - 4), top = clamp(rect.top - padding, 4, height - 4)
  return { left, top, width: Math.max(0, Math.min(width - 4, rect.left + rect.width + padding) - left), height: Math.max(0, Math.min(height - 4, rect.top + rect.height + padding) - top) }
}
function overlap(a: TourRect, b: TourRect) {
  return Math.max(0, Math.min(a.left + a.width, b.left + b.width) - Math.max(a.left, b.left)) * Math.max(0, Math.min(a.top + a.height, b.top + b.height) - Math.max(a.top, b.top))
}
export function placeTourCard(targets: TourRect[], viewport: { width: number; height: number }, card: { width: number; height: number }) {
  const width = Math.min(card.width, viewport.width - 24), height = Math.min(card.height, viewport.height - 24)
  const anchor = targets[0]
  if (!anchor) return { left: Math.max(12, (viewport.width - width) / 2), top: Math.max(12, (viewport.height - height) / 2), width, height, arrow: null }
  const centerX = anchor.left + anchor.width / 2, centerY = anchor.top + anchor.height / 2, gap = 16
  const candidates = [
    { left: anchor.left + anchor.width + gap, top: centerY - height / 2 },
    { left: anchor.left - width - gap, top: centerY - height / 2 },
    { left: centerX - width / 2, top: anchor.top + anchor.height + gap },
    { left: centerX - width / 2, top: anchor.top - height - gap },
    { left: 12, top: viewport.height - height - 12 },
    { left: viewport.width - width - 12, top: 12 },
  ].map(p => ({ left: clamp(p.left, 12, viewport.width - width - 12), top: clamp(p.top, 12, viewport.height - height - 12), width, height }))
  candidates.sort((a, b) => {
    const score = (p: TourRect) => targets.reduce((sum, target) => sum + overlap(p, target) * 100, 0) + Math.hypot(p.left + width / 2 - centerX, p.top + height / 2 - centerY)
    return score(a) - score(b)
  })
  const position = candidates[0]!
  const arrow = position.left >= anchor.left + anchor.width ? { side: "left", offset: clamp(centerY - position.top, 22, height - 22) }
    : position.left + width <= anchor.left ? { side: "right", offset: clamp(centerY - position.top, 22, height - 22) }
    : position.top >= anchor.top + anchor.height ? { side: "top", offset: clamp(centerX - position.left, 22, width - 22) }
    : position.top + height <= anchor.top ? { side: "bottom", offset: clamp(centerX - position.left, 22, width - 22) } : null
  return { ...position, arrow }
}
