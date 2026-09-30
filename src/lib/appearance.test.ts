import { describe, expect, it } from "vitest"
import { contrastRatio, defaultCustomTheme, resolveAppearance, themePresets, validCustomTheme } from "./appearance"

describe("appearance palettes", () => {
  it("keeps every preset readable while using the supplied colors", () => {
    for (const preset of themePresets) {
      const resolved = resolveAppearance(preset.id, null)!
      expect(resolved.background).toBe(preset.colors.background)
      expect(resolved.tokens["--primary"]).toBe(preset.colors.accent)
      expect(resolved.tokens["--theme-surface"]).toBe(preset.colors.surface)
      expect(resolved.tokens["--theme-detail"]).toBe(preset.colors.detail)
      expect(contrastRatio(resolved.background, resolved.tokens["--foreground"]!)).toBeGreaterThanOrEqual(4.5)
      expect(contrastRatio(resolved.tokens["--primary"]!, resolved.tokens["--primary-foreground"]!)).toBeGreaterThanOrEqual(4.5)
    }
  })

  it("rejects incomplete colors and still chooses readable text for low-contrast detail", () => {
    expect(validCustomTheme({ ...defaultCustomTheme, accent: "red" })).toBe(false)
    expect(resolveAppearance("custom", null)).toBeNull()
    const colors = { background: "#FFFFFF", surface: "#F4F4F4", accent: "#EEEEEE", detail: "#FAFAFA" }
    const resolved = resolveAppearance("custom", colors)!
    expect(contrastRatio(colors.background, resolved.tokens["--foreground"]!)).toBeGreaterThanOrEqual(4.5)
    expect(contrastRatio(colors.accent, resolved.tokens["--primary-foreground"]!)).toBeGreaterThanOrEqual(4.5)
  })
})
