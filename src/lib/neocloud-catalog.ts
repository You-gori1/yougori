import type { NeocloudOffer } from "@/api/neocloud-api"

export const offerKey = (offer: NeocloudOffer) => JSON.stringify([offer.provider, offer.product, offer.offerId, offer.location, offer.platform])
export interface CatalogFilter { search: string; minVram: number; minRam: number; minCpu: number; maxHourly: number; availableOnly: boolean }
export function filterOffers(offers: NeocloudOffer[], filter: CatalogFilter): NeocloudOffer[] {
  return offers.filter(o => {
    if (!`${o.provider} ${o.name} ${o.offerId} ${o.location ?? ""}`.toLowerCase().includes(filter.search.toLowerCase().trim())) return false
    if (filter.availableOnly && o.available !== true) return false
    if (filter.minVram > 0 && (o.vramGb == null || o.vramGb < filter.minVram)) return false
    if (filter.minRam > 0 && (o.memoryGb == null || o.memoryGb < filter.minRam)) return false
    if (filter.minCpu > 0 && (o.cpuCores == null || o.cpuCores < filter.minCpu)) return false
    if (filter.maxHourly > 0 && (o.hourlyUsd == null || o.hourlyUsd > filter.maxHourly)) return false
    return true
  }).sort((a, b) => Number(a.available === false) - Number(b.available === false)
    || (a.hourlyUsd ?? Infinity) - (b.hourlyUsd ?? Infinity) || a.provider.localeCompare(b.provider) || a.name.localeCompare(b.name))
}
export function quotePrice(offer: NeocloudOffer): string {
  const currency = offer.currency || "currency unreported"
  if (offer.hourlyPrice != null) return `${currency} ${offer.hourlyPrice.toLocaleString(undefined, { maximumFractionDigits: 4 })}/hr`
  if (offer.monthlyPrice != null) return `${currency} ${offer.monthlyPrice.toLocaleString(undefined, { maximumFractionDigits: 2 })}/month`
  return "Price in provider account"
}
