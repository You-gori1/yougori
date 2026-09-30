import { describe, expect, it } from "vitest"
import { filterOffers, offerKey, quotePrice } from "./neocloud-catalog"
import type { NeocloudOffer } from "@/api/neocloud-api"

export const sampleOffer = (patch: Partial<NeocloudOffer> = {}): NeocloudOffer => ({ provider: "runpod", product: "gpu", offerId: "h100", name: "H100", location: "US", hourlyPrice: 2, monthlyPrice: null, currency: "USD", hourlyUsd: 2, available: true, gpuCount: 1, vramGb: 80, cpuCores: 16, memoryGb: 128, storageGb: null, platform: null, ...patch })
const filters = { search: "", minVram: 0, minRam: 0, minCpu: 0, maxHourly: 0, availableOnly: false }
describe("neocloud comparison", () => {
  it("ranks comparable hourly prices, leaving unknown currency/monthly and unavailable offers separate", () => {
    const usd = sampleOffer(), foreign = sampleOffer({ provider: "jarvis", currency: "INR", hourlyPrice: 1, hourlyUsd: null }), monthly = sampleOffer({ provider: "latitude", monthlyPrice: 500, hourlyPrice: null, hourlyUsd: null }), unavailable = sampleOffer({ provider: "vast", hourlyUsd: .1, available: false })
    expect(filterOffers([unavailable, monthly, foreign, usd], filters)).toEqual([usd, foreign, monthly, unavailable])
    expect(quotePrice(foreign)).toBe("INR 1/hr")
    expect(quotePrice(monthly)).toBe("USD 500/month")
  })
  it("does not treat unknown stock or specs as confirmed capacity", () => {
    const unknown = sampleOffer({ available: null, vramGb: null, cpuCores: null, memoryGb: null })
    for (const filter of [{ availableOnly: true }, { minVram: 24 }, { minRam: 8 }, { minCpu: 2 }]) expect(filterOffers([unknown], { ...filters, ...filter })).toEqual([])
    expect(filterOffers([sampleOffer({ hourlyUsd: null })], { ...filters, maxHourly: 3 })).toEqual([])
    expect(filterOffers([sampleOffer()], { ...filters, search: "H100", minVram: 80 })).toHaveLength(1)
  })
  it("distinguishes identical hardware in different providers, regions and platforms", () => {
    const offer = sampleOffer()
    for (const patch of [{ provider: "prime" }, { location: "EU" }, { platform: "gpu-h100" }]) expect(offerKey(sampleOffer(patch))).not.toBe(offerKey(offer))
    expect(quotePrice(sampleOffer({ hourlyPrice: null, monthlyPrice: null }))).toBe("Price in provider account")
  })
})
