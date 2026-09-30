// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react"
import { afterEach, expect, it, vi } from "vitest"
import { NeocloudDeploymentCard } from "./neocloud-deployment-card"
const fixture = vi.hoisted(() => ({ provider: "prime" }))
vi.mock("@/context/platform-context", () => ({ usePlatform: () => ({ state: { environments: [{ id: "neo", name: "training" }], neocloudDeployments: { neo: { provider: fixture.provider, product: "gpu", name: "training", resourceId: "1", state: "Running", image: "base", offer: "h100" } } }, refreshPlatform: async () => {} }) }))
vi.mock("@/components/dialogs/cloud-environment-dialog", () => ({ CloudEnvironmentDialog: () => null }))
afterEach(cleanup)
it("does not offer stop/start for providers that only support deletion", () => {
  fixture.provider = "prime"
  render(<NeocloudDeploymentCard environmentId="neo" />)
  expect(screen.getByRole("heading").textContent).toContain("Prime Intellect")
  expect(screen.queryByRole("button", { name: "Start compute" })).toBeNull()
  expect(screen.queryByRole("button", { name: "Stop compute" })).toBeNull()
  expect(screen.getByText("Delete provider resource")).toBeTruthy()
})
it("labels Civo power-off billing and exposes reversible controls", () => {
  fixture.provider = "civo"
  render(<NeocloudDeploymentCard environmentId="neo" />)
  expect(screen.getByRole("button", { name: "Power off (still billed)" })).toBeTruthy()
  expect(screen.getByRole("button", { name: "Start compute" }).hasAttribute("disabled")).toBe(true)
})
