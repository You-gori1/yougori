// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest"
import { changeTour, claimWindowTour, getTour, ownsTour, overviewSteps, parseTour, practiceSteps, prepareTourWindow, startInstructions, stopInstructions, tourEnvironmentCreated, tourInstallerStarted, tourTerminalOpened, tourWindowFailed, trackTourCreation } from "./instructions-tour"
import { returnTourHome, tourPortAdded, tourWebsiteOpened, tourWebsitePublished, tourEnvironmentCreationFailed } from "./instructions-tour"

beforeEach(() => { localStorage.clear(); startInstructions() })
describe("instructions tour state", () => {
  it("starts explicitly with the complete overview before practice", () => {
    expect(getTour()).toMatchObject({ active: true, step: "welcome", revision: 0 })
    expect(ownsTour(getTour())).toBe(true)
    expect(overviewSteps.at(-1)).toBe("windows-overview")
    expect(overviewSteps.slice(5, 7)).toEqual(["service-ports", "connections"])
    expect(overviewSteps.filter(step => step === "service-ports")).toHaveLength(1)
    expect(practiceSteps[0]).toBe("create-open")
  })
  it("validates saved state and ignores expired, malformed and unknown values", () => {
    const valid = getTour()!
    expect(parseTour(valid)).toEqual(valid)
    for (const invalid of [null, {}, { ...valid, version: 99 }, { ...valid, step: "delete-everything" }, { ...valid, revision: -1 }, { ...valid, environmentId: '<script>' }, { ...valid, owner: "" }, { ...valid, updated: Date.now() - 86_400_001 }, { ...valid, updated: Date.now() + 120_000 }, { ...valid, pendingName: "x".repeat(81) }, { ...valid, handoffAt: NaN }]) expect(parseTour(invalid)).toBeNull()
  })
  it("advances only for the matching successful creation", () => {
    tourEnvironmentCreated("env-unrelated", "Unrelated")
    expect(getTour()?.step).toBe("welcome")
    changeTour({ step: "create-submit" }); trackTourCreation("First container")
    tourEnvironmentCreated("env-wrong", "Wrong name")
    expect(getTour()?.step).toBe("create-submit")
    tourEnvironmentCreated("env-first", "First container")
    expect(getTour()).toMatchObject({ step: "created", environmentId: "env-first", pendingName: undefined })
  })
  it("Skip does not leave any delayed creation or installer callback active", () => {
    changeTour({ step: "create-submit" }); trackTourCreation("First container"); stopInstructions()
    tourEnvironmentCreated("env-first", "First container"); tourInstallerStarted("env-first", "codex")
    expect(getTour()).toMatchObject({ active: false, step: "create-submit" })
    expect(getTour()?.environmentId).toBeUndefined()
  })
  it("clears a failed pending creation without advancing or affecting another request", () => {
    changeTour({ step: "create-submit" }); trackTourCreation("First container")
    tourEnvironmentCreationFailed("Unrelated")
    expect(getTour()?.pendingName).toBe("First container")
    tourEnvironmentCreationFailed("First container")
    expect(getTour()).toMatchObject({ step: "create-submit", pendingName: undefined })
    expect(getTour()?.environmentId).toBeUndefined()
  })
  it("hands off only the selected environment and restores the action on failure", () => {
    changeTour({ step: "start", environmentId: "env-first" })
    prepareTourWindow("env-other"); expect(getTour()?.step).toBe("start")
    prepareTourWindow("env-first"); expect(getTour()?.step).toBe("first-window")
    claimWindowTour("env-first"); expect(getTour()?.step).toBe("first-window") // current owner cannot steal a handoff
    tourWindowFailed("env-first"); expect(getTour()).toMatchObject({ step: "start", handoffAt: undefined })
    changeTour({ step: "new-window" }); prepareTourWindow("env-first"); expect(getTour()?.step).toBe("second-window")
    tourWindowFailed("env-first"); expect(getTour()?.step).toBe("new-window")
  })
  it("does not claim an unrelated or already open viewer", () => {
    changeTour({ step: "first-window", environmentId: "env-first", owner: "other-window", handoffAt: Date.now() + 1 })
    claimWindowTour("env-other"); claimWindowTour("env-first")
    expect(getTour()?.owner).toBe("other-window")
  })
  it("tracks command dispatch, not an assumed installation success", () => {
    changeTour({ step: "install", environmentId: "env-first" })
    tourInstallerStarted("env-first", "claude"); tourInstallerStarted("env-other", "codex")
    expect(getTour()?.step).toBe("install")
    tourInstallerStarted("env-first", "codex"); expect(getTour()?.step).toBe("install-running")
    tourTerminalOpened("env-first"); expect(getTour()?.step).toBe("install-running")
    changeTour({ step: "new-terminal" }); tourTerminalOpened("env-first"); expect(getTour()?.step).toBe("run-codex")
  })
  it("returns from the website terminal and follows only its successful quick publication", () => {
    changeTour({ step: "demo-start", environmentId: "env-first" })
    returnTourHome(); expect(getTour()?.step).toBe("demo-start")
    changeTour({ step: "demo-return" }); returnTourHome()
    expect(getTour()).toMatchObject({ step: "demo-port-open", owner: getTour()!.home })
    changeTour({ step: "demo-port-add" })
    tourPortAdded("env-other", 3000); tourPortAdded("env-first", 8080)
    expect(getTour()?.step).toBe("demo-port-add")
    tourPortAdded("env-first", 3000)
    const publication = { port: 3000, kind: "cloudflare", status: "active", cloudflareAccount: false, urls: ["https://demo.trycloudflare.com"] }
    for (const other of [{ ...publication, port: 8080 }, { ...publication, kind: "local" }, { ...publication, cloudflareAccount: true }, { ...publication, status: "error" }, { ...publication, urls: [] }]) tourWebsitePublished("env-first", other)
    tourWebsitePublished("env-other", publication)
    expect(getTour()?.step).toBe("demo-publish")
    tourWebsitePublished("env-first", publication)
    expect(getTour()?.step).toBe("demo-link")
    tourWebsiteOpened("env-other", 3000, true); tourWebsiteOpened("env-first", 3000, false)
    expect(getTour()?.step).toBe("demo-link")
    tourWebsiteOpened("env-first", 3000, true)
    expect(getTour()?.step).toBe("demo-visit")
    stopInstructions(); tourWebsitePublished("env-first", publication)
    expect(getTour()?.active).toBe(false)
  })
})
