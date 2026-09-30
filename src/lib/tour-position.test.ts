import { describe, expect, it } from "vitest"
import { clipTourRect, placeTourCard } from "./tour-position"

describe("guided tour positioning", () => {
  it("puts a nearby card beside its target without hiding it", () => {
    const target = { left: 450, top: 300, width: 120, height: 40 }
    const card = placeTourCard([target], { width: 1280, height: 900 }, { width: 348, height: 280 })
    expect(card.arrow).not.toBeNull()
    expect(card.left + card.width <= target.left || card.left >= target.left + target.width || card.top + card.height <= target.top || card.top >= target.top + target.height).toBe(true)
  })
  it.each([320, 768, 1280])("keeps the card inside a %s pixel viewport at every edge", width => {
    for (const left of [0, width / 2, width - 40]) for (const top of [0, 300, 690]) {
      const card = placeTourCard([{ left, top, width: 40, height: 40 }], { width, height: 720 }, { width: 348, height: 900 })
      expect(card.left).toBeGreaterThanOrEqual(12)
      expect(card.top).toBeGreaterThanOrEqual(12)
      expect(card.left + card.width).toBeLessThanOrEqual(width - 12)
      expect(card.top + card.height).toBeLessThanOrEqual(708)
    }
  })
  it("clips huge and offscreen highlights and centers missing targets", () => {
    expect(clipTourRect({ left: -100, top: -10, width: 2000, height: 900 }, 1000, 800)).toEqual({ left: 4, top: 4, width: 992, height: 792 })
    expect(clipTourRect({ left: 3000, top: 1000, width: 100, height: 100 }, 1000, 800).width).toBe(0)
    expect(placeTourCard([], { width: 1000, height: 800 }, { width: 300, height: 200 })).toMatchObject({ left: 350, top: 300, arrow: null })
  })
  it("does not cover a second highlighted control when there is room", () => {
    const primary = { left: 500, top: 700, width: 140, height: 40 }
    const extra = { left: 500, top: 400, width: 140, height: 40 }
    const card = placeTourCard([primary, extra], { width: 1280, height: 900 }, { width: 348, height: 260 })
    for (const target of [primary, extra]) expect(card.left + card.width <= target.left || card.left >= target.left + target.width || card.top + card.height <= target.top || card.top >= target.top + target.height).toBe(true)
  })
})
