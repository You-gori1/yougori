import { expect, it } from "vitest"
import jsQR from "jsqr"
import { createTerminalQr } from "./terminal-qr"

it.each(["https://example.com/setup?code=abc&next=%2Fhome#login", "http://localhost:3000/", "https://example.com/東京?name=Łukasz"])("QR modules decode to the exact link: %s", url => {
  const qr = createTerminalQr(url)!
  expect(qr).not.toBeNull()
  // Decode the exact SVG module path in memory. No screenshot or file capture.
  const scale = 4, width = qr.size * scale
  const pixels = new Uint8ClampedArray(width * width * 4).fill(255)
  for (const match of qr.path.matchAll(/M(\d+) (\d+)h1v1h-1z/g)) {
    const x = Number(match[1]) * scale, y = Number(match[2]) * scale
    for (let dy = 0; dy < scale; dy++) for (let dx = 0; dx < scale; dx++) {
      const offset = ((y + dy) * width + x + dx) * 4
      pixels[offset] = pixels[offset + 1] = pixels[offset + 2] = 0
    }
  }
  expect(jsQR(pixels, width, width)?.data).toBe(url)
})

it("bounds oversized QR input", () => {
  expect(createTerminalQr("a".repeat(2049))).toBeNull()
})
