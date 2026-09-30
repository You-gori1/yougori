// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest"
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, expect, it, vi } from "vitest"
import { VaultItemEditor } from "@/components/vault-item-editor"

afterEach(cleanup)

it("saves several items together through the in-app editor", async () => {
  const save = vi.fn().mockResolvedValue(undefined)
  render(<VaultItemEditor onSave={save} onCancel={vi.fn()} />)
  fireEvent.change(screen.getByRole("textbox", { name: "Item name" }), { target: { value: "First" } })
  fireEvent.change(screen.getByLabelText("Item value"), { target: { value: "first-secret" } })
  fireEvent.click(screen.getByRole("button", { name: "Add another" }))
  expect(screen.getByText("Ready to save · 1")).toBeInTheDocument()
  fireEvent.change(screen.getByRole("textbox", { name: "Item name" }), { target: { value: "Second" } })
  fireEvent.change(screen.getByLabelText("Item value"), { target: { value: "second-secret" } })
  fireEvent.click(screen.getByRole("button", { name: "Save items" }))
  await waitFor(() => expect(save).toHaveBeenCalledWith([
    { label: "First", kind: "password", resource: "", value: "first-secret" },
    { label: "Second", kind: "password", resource: "", value: "second-secret" },
  ]))
})

it("keeps line breaks in a pasted private key", async () => {
  const save = vi.fn().mockResolvedValue(undefined)
  render(<VaultItemEditor onSave={save} onCancel={vi.fn()} />)
  fireEvent.change(screen.getByRole("textbox", { name: "Item name" }), { target: { value: "SSH identity" } })
  fireEvent.change(screen.getByLabelText("Item type"), { target: { value: "private_key" } })
  const key = "-----BEGIN PRIVATE KEY-----\nexample\n-----END PRIVATE KEY-----"
  fireEvent.change(screen.getByRole("textbox", { name: "Item value" }), { target: { value: key } })
  fireEvent.click(screen.getByRole("button", { name: "Save item" }))
  await waitFor(() => expect(save).toHaveBeenCalledWith([{ label: "SSH identity", kind: "private_key", resource: "", value: key }]))
})
