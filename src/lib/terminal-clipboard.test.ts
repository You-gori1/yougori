import { describe, expect, it } from "vitest"
import { osc52CopyText, terminalClipboardAction } from "./terminal-clipboard"

const key = (key: string, options = {}) => ({ key, ctrlKey: true, metaKey: false, altKey: false, shiftKey: false, ...options })
describe("terminal clipboard shortcuts", () => {
  it("copies a selection but preserves Ctrl+C interrupts without one", () => {
    expect(terminalClipboardAction(key("c"), true)).toBe("copy")
    expect(terminalClipboardAction(key("c"), false)).toBeNull()
    expect(terminalClipboardAction(key("C", { shiftKey: true }), false)).toBe("copy")
  })
  it("handles Windows, Linux and Mac paste shortcuts", () => {
    expect(terminalClipboardAction(key("v"), false)).toBe("paste")
    expect(terminalClipboardAction(key("V", { shiftKey: true }), false)).toBe("paste")
    expect(terminalClipboardAction(key("Insert", { ctrlKey: false, shiftKey: true }), false)).toBe("paste")
    expect(terminalClipboardAction(key("v", { ctrlKey: false, metaKey: true }), false)).toBe("paste")
  })
  it("leaves ordinary typing, AltGr and other control keys alone", () => {
    expect(terminalClipboardAction(key("v", { ctrlKey: false }), true)).toBeNull()
    expect(terminalClipboardAction(key("v", { altKey: true }), true)).toBeNull()
    expect(terminalClipboardAction(key("d"), false)).toBeNull()
  })
})
describe("guest OSC 52 copies", () => {
  const encoded = btoa(String.fromCharCode(...new TextEncoder().encode("cd \"/srv/CRM PrompX\" && node dist/index.js — ok")))
  it("copies text a program sends right after the user's own click or key press", () => {
    expect(osc52CopyText("c;" + encoded, 150, true)).toBe("cd \"/srv/CRM PrompX\" && node dist/index.js — ok")
    expect(osc52CopyText(";" + encoded, 0, true)).toBe("cd \"/srv/CRM PrompX\" && node dist/index.js — ok")
  })
  it("ignores background, unfocused and clipboard-read requests", () => {
    expect(osc52CopyText("c;" + encoded, 2500, true)).toBeNull()
    expect(osc52CopyText("c;" + encoded, 100, false)).toBeNull()
    expect(osc52CopyText("c;?", 100, true)).toBeNull()
  })
  it("rejects malformed or oversized payloads", () => {
    expect(osc52CopyText("c;not base64!", 100, true)).toBeNull()
    expect(osc52CopyText("x;" + encoded, 100, true)).toBeNull()
    expect(osc52CopyText("c;" + btoa("\xff\xfe"), 100, true)).toBeNull()
    expect(osc52CopyText("c;" + "A".repeat(1024 * 1024), 100, true)).toBeNull()
  })
})
