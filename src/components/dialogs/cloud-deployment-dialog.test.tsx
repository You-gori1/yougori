// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { CloudDeploymentDialog } from "./cloud-deployment-dialog"
import { cloudOptionKinds, type CloudOptionKind } from "@/api/cloud-api"

const options = vi.hoisted(() => vi.fn())
const authenticate = vi.hoisted(() => vi.fn())
const deployCloudEnvironment = vi.hoisted(() => vi.fn())
vi.mock("@/api/cloud-api", async importOriginal => {
  const actual = await importOriginal<typeof import("@/api/cloud-api")>()
  return { ...actual, cloudApi: { ...actual.cloudApi, options, authenticate } }
})
vi.mock("@/context/platform-context", () => ({ usePlatform: () => ({ deployCloudEnvironment }) }))

const fields: Record<CloudOptionKind, string> = { regions: "region", images: "image", machineTypes: "machineType", subnets: "subnet", securityGroups: "securityGroup", keyPairs: "keyPair", resourceGroups: "resourceGroup", sshKeys: "sshPublicKey" }
beforeEach(() => {
  authenticate.mockReset().mockResolvedValue({})
  deployCloudEnvironment.mockReset().mockResolvedValue({})
  options.mockReset()
  options.mockImplementation(async (_provider: string, _account: string, _region: string, kind: CloudOptionKind) => {
    if (kind === "securityGroups") throw new Error("AccessDenied")
    const items = kind === "sshKeys" ? [{ id: "ssh-ed25519 AAAAkey", name: "id_ed25519.pub", detail: "me@pc" }] : [{ id: `${kind}-1`, name: `${kind} one`, detail: "detail" }]
    return { kind, field: fields[kind], items, imageProject: kind === "images" ? "ubuntu-os-cloud" : undefined }
  })
})
afterEach(cleanup)

describe("cloud deployment choices", () => {
  it("requires risk acknowledgement after authentication and resets it after edits", async () => {
    vi.stubGlobal("PointerEvent", MouseEvent)
    render(<CloudDeploymentDialog embedded />)
    expect(screen.getByRole("complementary", { name: "Cloud deployment risks" }).textContent).toContain("Charges can continue after a failure")
    fireEvent.change(screen.getByRole("textbox", { name: "AWS SSO profile" }), { target: { value: "test-profile" } })
    fireEvent.click(screen.getByRole("button", { name: "Authenticate in browser" }))
    await waitFor(() => expect(authenticate).toHaveBeenCalled())
    const submit = screen.getByRole("button", { name: "Create cloud environment" }) as HTMLButtonElement
    expect(submit.disabled).toBe(true)
    fireEvent.click(screen.getByRole("checkbox"))
    await waitFor(() => expect(submit.disabled).toBe(false))
    fireEvent.change(screen.getByRole("textbox", { name: "Environment name" }), { target: { value: "test-project" } })
    expect(submit.disabled).toBe(true)
    fireEvent.click(screen.getByRole("checkbox"))
    fireEvent.click(submit)
    await waitFor(() => expect(deployCloudEnvironment).toHaveBeenCalledWith(expect.objectContaining({ name: "test-project" }), true))
  })
  it("chooses lookups per provider, adding sizes once a region is known", () => {
    expect(cloudOptionKinds("aws", "")).toEqual(["regions", "images", "subnets", "securityGroups", "keyPairs"])
    expect(cloudOptionKinds("azure", "westeurope")).toEqual(["regions", "images", "subnets", "securityGroups", "resourceGroups", "sshKeys", "machineTypes"])
    expect(cloudOptionKinds("google", "")).toEqual(["regions", "images", "sshKeys"])
    expect(cloudOptionKinds("google", "europe-west1-b")).toContain("subnets")
  })

  it("offers looked-up values as suggestions, reports failed lookups and fills local SSH keys", async () => {
    render(<CloudDeploymentDialog embedded />)
    fireEvent.click(screen.getByRole("button", { name: "Azure" }))
    fireEvent.change(screen.getByLabelText("Azure subscription ID"), { target: { value: "sub-1" } })
    fireEvent.click(screen.getByRole("button", { name: "Look up choices" }))
    await waitFor(() => expect(options).toHaveBeenCalledWith("azure", "sub-1", "", "resourceGroups"))
    expect((await screen.findByRole("alert")).textContent).toContain("securityGroups: Error: AccessDenied")
    const region = screen.getByLabelText("Region") as HTMLInputElement
    expect(region.getAttribute("list")).toBe("cloud-choices-region")
    expect(document.querySelector("#cloud-choices-region option")?.getAttribute("value")).toBe("regions-1")
    expect(document.querySelector("#cloud-choices-machineType")).toBeNull()
    fireEvent.click(screen.getByRole("button", { name: "Use id_ed25519.pub" }))
    expect((screen.getByPlaceholderText("ssh-ed25519 AAAA…") as HTMLTextAreaElement).value).toBe("ssh-ed25519 AAAAkey")
    fireEvent.change(region, { target: { value: "westeurope" } })
    fireEvent.click(screen.getByRole("button", { name: "Refresh choices" }))
    await waitFor(() => expect(options).toHaveBeenCalledWith("azure", "sub-1", "westeurope", "machineTypes"))
  })
})
