import { expect, it } from "vitest"
import { averageSpeed, sumDays, usageByDay } from "./model-usage"

const counters = (requests: number, extra = {}) => ({ requests, prompt_tokens: requests * 10, completion_tokens: requests * 5, errors: 0, rejected: 0, api: requests, yougori: 0, ...extra })
const hour = (date: Date) => String(Math.floor(date.getTime() / 3_600_000))

it("groups hourly usage into local days and ignores hours outside the range", () => {
  const now = new Date(2026, 8, 25, 15)
  const usage = { since: 0, totals: counters(0), recent: [], hours: {
    [hour(new Date(2026, 8, 25, 1))]: counters(2), [hour(new Date(2026, 8, 25, 14))]: counters(3, { errors: 1 }),
    [hour(new Date(2026, 8, 23, 9))]: counters(4), [hour(new Date(2026, 7, 1, 9))]: counters(100),
  } }
  const days = usageByDay(usage, 7, now)
  expect(days).toHaveLength(7)
  expect(days.map(day => day.requests)).toEqual([0, 0, 0, 0, 4, 0, 5])
  expect(days[6]!.date).toEqual(new Date(2026, 8, 25))
  expect(sumDays(days)).toMatchObject({ requests: 9, prompt_tokens: 90, completion_tokens: 45, errors: 1, api: 9 })
})

it("averages speed over completed replies only", () => {
  const request = (outcome: "ok" | "error", completion_tokens: number, seconds: number) => ({ time: 0, source: "api" as const, outcome, prompt_tokens: 0, completion_tokens, seconds, stream: false })
  expect(averageSpeed([request("ok", 100, 4), request("ok", 50, 1), request("error", 0, 9)])).toBe(30)
  expect(averageSpeed([])).toBeNull()
})
