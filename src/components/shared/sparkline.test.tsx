import { renderToStaticMarkup } from "react-dom/server"
import { describe, expect, it } from "vitest"
import { Sparkline } from "./sparkline"

const chart = (values: number[], unavailable = false) => renderToStaticMarkup(<Sparkline label="Recent CPU usage" values={values} unavailable={unavailable} />)

describe("resource sparklines", () => {
  it("renders an accessible, solid-filled curve on a fixed percent scale", () => {
    const html = chart([0, 50, 100])
    expect(html).toContain('aria-label="Recent CPU usage"')
    expect(html).toContain('role="img"')
    expect(html).toContain("M 3 29 C 46.5 29, 46.5 16, 90 16 C 133.5 16, 133.5 3, 177 3")
    expect(html).toContain('fill-opacity="0.09"')
    expect(html).not.toContain("Gradient")
  })

  it("does not amplify low readings to the chart ceiling", () => {
    expect(chart([10, 10])).toContain("M 3 26.4 C 90 26.4, 90 26.4, 177 26.4")
  })

  it("does not invent readings for missing or unavailable metrics", () => {
    expect(chart([])).toContain("Waiting for usage samples")
    expect(chart([])).not.toContain("<path")
    expect(chart([30, 40], true)).toContain("Usage unavailable")
    expect(chart([30, 40], true)).not.toContain("<path")
  })

  it("keeps a lone sample at the latest position without inventing a history", () => {
    const html = chart([50])
    expect(html).toContain('d="M 177 16"')
    expect(html).not.toContain('fill-opacity="0.09"')
  })

  it("bounds invalid samples and limits rendering work", () => {
    const html = chart([NaN, -20, Infinity, 200])
    expect(html).not.toMatch(/NaN|Infinity/)
    expect(html).toContain("M 3 29 C 90 29, 90 3, 177 3")
    expect(chart(Array(1000).fill(50)).match(/ C /g)).toHaveLength(238)
  })
})
