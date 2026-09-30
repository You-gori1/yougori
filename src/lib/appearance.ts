import type { CustomThemeColors, ThemePreference } from "@/types/platform"

export const themePresets = [
  { id: "theme1", name: "Clay", colors: { background: "#F3E4C9", surface: "#BFA28C", accent: "#A98B76", detail: "#BABF94" } },
  { id: "theme2", name: "Garnet", colors: { background: "#2D0000", surface: "#6D0808", accent: "#757D6F", detail: "#EEEAD7" } },
  { id: "theme3", name: "Coast", colors: { background: "#F5EBDD", surface: "#F2765E", accent: "#315B8C", detail: "#413333" } },
  { id: "theme4", name: "Midnight", colors: { background: "#010736", surface: "#0D1C42", accent: "#22396F", detail: "#FCF1D0" } },
  { id: "theme5", name: "Rose", colors: { background: "#FFF5F5", surface: "#F7D6D0", accent: "#E2B4BD", detail: "#4A4A4A" } },
] as const satisfies ReadonlyArray<{ id: ThemePreference; name: string; colors: CustomThemeColors }>

export const defaultCustomTheme: CustomThemeColors = {
  background: "#F5EBDD", surface: "#D8C4AD", accent: "#315B8C", detail: "#413333",
}

export function validThemeColor(value: string): boolean {
  return /^#[\da-fA-F]{6}$/.test(value)
}

export function validCustomTheme(colors: CustomThemeColors | null | undefined): colors is CustomThemeColors {
  return Boolean(colors && validThemeColor(colors.background) && validThemeColor(colors.surface) && validThemeColor(colors.accent) && validThemeColor(colors.detail))
}

function rgb(hex: string): [number, number, number] {
  return [1, 3, 5].map(index => Number.parseInt(hex.slice(index, index + 2), 16)) as [number, number, number]
}

function mix(first: string, second: string, secondWeight: number): string {
  const a = rgb(first), b = rgb(second)
  return `#${a.map((value, index) => Math.round(value * (1 - secondWeight) + b[index]! * secondWeight).toString(16).padStart(2, "0")).join("")}`
}

export function contrastRatio(first: string, second: string): number {
  const luminance = (hex: string) => {
    const [r, g, b] = rgb(hex).map(value => {
      const channel = value / 255
      return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4
    })
    return r! * 0.2126 + g! * 0.7152 + b! * 0.0722
  }
  const a = luminance(first), b = luminance(second)
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05)
}

function readableText(background: string, preferred?: string): string {
  if (preferred && contrastRatio(background, preferred) >= 4.5) return preferred
  if (contrastRatio(background, "#161616") >= 4.5) return "#161616"
  return contrastRatio(background, "#FFFFFF") >= 4.5 ? "#FFFFFF" : "#000000"
}

export interface ResolvedAppearance {
  dark: boolean
  background: string
  tokens: Record<string, string>
}

export function resolveAppearance(theme: ThemePreference, custom: CustomThemeColors | null | undefined): ResolvedAppearance | null {
  const colors = theme === "custom" ? custom : themePresets.find(preset => preset.id === theme)?.colors
  if (!validCustomTheme(colors)) return null
  const { background, surface, accent, detail } = colors
  const dark = contrastRatio(background, "#FFFFFF") > contrastRatio(background, "#161616")
  const foreground = readableText(background, detail)
  const card = mix(background, surface, dark ? 0.74 : 0.28)
  const popover = mix(background, surface, dark ? 0.85 : 0.38)
  const muted = mix(background, surface, dark ? 0.53 : 0.2)
  const mutedCandidate = mix(foreground, background, 0.2)
  const mutedForeground = contrastRatio(muted, mutedCandidate) >= 4.5 ? mutedCandidate : foreground
  return {
    dark, background,
    tokens: {
      "--background": background,
      "--foreground": foreground,
      "--card": card,
      "--card-foreground": readableText(card, foreground),
      "--popover": popover,
      "--popover-foreground": readableText(popover, foreground),
      "--primary": accent,
      "--primary-foreground": readableText(accent, detail),
      "--ring": accent,
      "--accent": mix(background, detail, dark ? 0.2 : 0.19),
      "--accent-foreground": foreground,
      "--secondary": mix(background, surface, dark ? 0.58 : 0.35),
      "--secondary-foreground": foreground,
      "--muted": muted,
      "--muted-foreground": mutedForeground,
      "--border": mix(background, foreground, dark ? 0.22 : 0.21),
      "--input": mix(background, foreground, dark ? 0.27 : 0.25),
      "--graph-line": mix(background, foreground, 0.48),
      "--graph-dot": detail,
      "--theme-surface": surface,
      "--theme-detail": detail,
      "--dashboard-primary": accent,
      "--dashboard-primary-hover": mix(accent, foreground, dark ? 0.12 : 0.16),
      "--dashboard-primary-foreground": readableText(accent, detail),
    },
  }
}
