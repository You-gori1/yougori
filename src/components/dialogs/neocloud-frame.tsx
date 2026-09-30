import { useEffect, useState, type ReactNode } from "react"
import { money, RUNPOD_CONSOLE, type RunpodAccount } from "@/api/runpod-api"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Heading, Lines, StepRail, WebLink, Workbench } from "./neocloud-ui"

/** One creation flow: its steps and page, what the summary shows, and how it is created. */
export interface Order {
  kicker: string
  title: string
  lede: string
  steps: string[]
  step: number
  setStep(step: number): void
  /** Whether each step's choices are complete. */
  complete: boolean[]
  main: ReactNode
  summary: [string, ReactNode | null | undefined][]
  price?: { amount: number | null; unit: "hour" | "month"; lines: string[]; empty: string }
  consent: string
  submitLabel: string
  /** Everything but the consent is ready. */
  ready: boolean
  /** Changes reset the consent, so a changed order is reviewed again. */
  signature: string
  submit(): void
}

export function AccountPanel({ account }: { account: RunpodAccount | null | undefined }) {
  if (!account) return null
  return <section className="neo-panel" aria-label="Account">
    <p className="neo-h3">Balance</p>
    <p className="neo-balance"><strong>{money(account.balance)}</strong>{account.spendPerHour ? <span>spending {money(account.spendPerHour)}/hr</span> : null}</p>
    <p className="neo-links"><span>{account.email}</span><WebLink url={`${RUNPOD_CONSOLE}/user/billing`}>Add funds</WebLink></p>
  </section>
}

function PricePanel({ price, balance }: { price: NonNullable<Order["price"]>; balance: number | null }) {
  const hours = price.unit === "hour" && price.amount && balance != null ? balance / price.amount : null
  return <section className="neo-panel" aria-label="Price">
    <p className="neo-h3">{price.unit === "hour" ? "While it runs" : "Every month"}</p>
    {price.amount != null
      ? <p className="neo-price"><strong>{money(price.amount)}<small>/{price.unit === "hour" ? "hr" : "month"}</small></strong>{price.lines.map(line => <span key={line}>{line}</span>)}</p>
      : <p className="neo-note">{price.empty}</p>}
    {hours != null ? <>
      <div className={`neo-runway${hours < 2 ? " is-low" : ""}`} aria-hidden="true"><i style={{ width: `${Math.min(100, (hours / 24) * 100)}%` }} /></div>
      <p className="neo-note">{hours < 1 ? `Your balance covers about ${Math.max(1, Math.floor(hours * 60))} minutes.` : `Your balance covers about ${hours >= 48 ? `${Math.floor(hours / 24)} days` : `${Math.floor(hours)} hours`}.`}</p>
    </> : null}
  </section>
}

/** The workbench around a flow: steps and page, live summary and price, and the action bar. */
export function OrderFrame({ order, account, busy, error, onBack }: { order: Order; account: RunpodAccount | null | undefined; busy: boolean; error?: string; onBack(): void }) {
  const [agreed, setAgreed] = useState(false)
  useEffect(() => { setAgreed(false) }, [order.signature])
  const last = order.step === order.steps.length - 1
  const reachable = (index: number) => order.complete.slice(0, index).every(Boolean)
  return <Workbench
    main={<>
      <button type="button" className="neo-link neo-note" disabled={busy} onClick={onBack}>All Neocloud options</button>
      <div className="mt-3"><Heading kicker={order.kicker} title={order.title}>{order.lede}</Heading></div>
      <StepRail steps={order.steps} current={order.step} reachable={reachable} onJump={order.setStep} />
      {order.main}
    </>}
    side={<>
      {error ? <p role="alert" className="neo-panel neo-error">{error}</p> : null}
      {order.price ? <PricePanel price={order.price} balance={account?.balance ?? null} /> : null}
      <section className="neo-panel" aria-label="Your choices"><p className="neo-h3">Summary</p><Lines rows={order.summary} /></section>
      <AccountPanel account={account} />
    </>}
    footer={<>
      <Button type="button" variant="ghost" disabled={busy || order.step === 0} onClick={() => order.setStep(order.step - 1)}>Back</Button>
      {last ? <label className="neo-consent"><Checkbox checked={agreed} disabled={!order.ready || busy} onCheckedChange={v => setAgreed(v === true)} /><span>{order.consent}</span></label> : <span className="neo-footer-spacer" />}
      {last ? <span className="neo-footer-spacer" /> : null}
      {order.price?.amount != null ? <span className="neo-footer-price"><strong>{money(order.price.amount)}</strong>/{order.price.unit === "hour" ? "hr" : "month"}</span> : null}
      {last
        ? <Button type="button" className="neo-cta" disabled={busy || !order.ready || !agreed} loading={busy} onClick={order.submit}>{order.submitLabel}</Button>
        : <Button type="button" className="neo-cta" disabled={!order.complete[order.step]} onClick={() => order.setStep(order.step + 1)}>Continue</Button>}
    </>}
  />
}
