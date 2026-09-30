// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest"
import { cleanup, render, screen, waitFor } from "@testing-library/react"
import { afterEach, expect, it, vi } from "vitest"
import { ModelNodeProgress } from "./model-node-progress"
import type { Environment } from "@/types/platform"

const { status, logs } = vi.hoisted(() => ({ status: vi.fn(), logs: vi.fn() }))
vi.mock("@/api/projects-api", () => ({ modelsApi: { status } }))
vi.mock("@/api/workspace-api", () => ({ workspaceApi: { logs } }))
afterEach(() => { cleanup(); vi.resetAllMocks() })

const environment = { id: "model-1", name: "Local model", status: "provisioning", lastError: null } as unknown as Environment

it("shows setup progress before container logs exist", async () => {
  logs.mockRejectedValue(new Error("no such container"))
  render(<ModelNodeProgress environment={environment} />)
  expect(screen.getByLabelText("Model setup for Local model")).toHaveTextContent("Preparing CUDA runtime and container image")
  expect(screen.getByLabelText("Model setup logs")).toHaveTextContent("Waiting for container logs")
  await waitFor(() => expect(logs).toHaveBeenCalledWith("model-1"))
  expect(status).not.toHaveBeenCalled()
})

it("shows live model download logs", async () => {
  status.mockResolvedValue({ status: "downloading" })
  logs.mockResolvedValue("Downloading weights: 50%")
  render(<ModelNodeProgress environment={{ ...environment, status: "running" }} />)
  expect(await screen.findByText("Downloading model weights")).toBeInTheDocument()
  expect(screen.getByLabelText("Model setup logs")).toHaveTextContent("Downloading weights: 50%")
})

it("removes setup logs when the model is ready", async () => {
  status.mockResolvedValue({ status: "ready" })
  logs.mockResolvedValue("Model ready")
  render(<ModelNodeProgress environment={{ ...environment, status: "running" }} />)
  await waitFor(() => expect(screen.queryByLabelText("Model setup logs")).not.toBeInTheDocument())
})
