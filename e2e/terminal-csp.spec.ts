import { readFileSync } from "node:fs"
import { test, expect, type Page } from "@playwright/test"
import { terminalTheme } from "../src/lib/terminal-theme"

const security = JSON.parse(readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8")).app.security
const xterm = readFileSync(new URL("../node_modules/@xterm/xterm/lib/xterm.js", import.meta.url))
const css = readFileSync(new URL("../node_modules/@xterm/xterm/css/xterm.css", import.meta.url))

async function renderTerminal(page: Page, injectStyleNonce: boolean) {
  // Tauri's release asset handler appends a style nonce unless this directive
  // is excluded. A nonce makes Chromium ignore style-src 'unsafe-inline'.
  // Dev-server tests alone never exercise this release-only transformation.
  const directives = new Map<string, string>(security.csp.split(";").map((value: string) => {
    const [name, ...sources] = value.trim().split(/\s+/)
    return [name, sources.join(" ")]
  }))
  if (injectStyleNonce) directives.set("style-src", `${directives.get("style-src")} 'nonce-terminal-test'`)
  directives.set("script-src", `${directives.get("script-src") ?? directives.get("default-src")} 'nonce-terminal-test'`)
  const csp = [...directives].map(([name, sources]) => `${name} ${sources}`).join("; ")
  await page.route("**/terminal-csp/**", route => {
    const name = new URL(route.request().url()).pathname.split("/").pop()
    if (name === "xterm.js") return route.fulfill({ contentType: "text/javascript", body: xterm })
    if (name === "xterm.css") return route.fulfill({ contentType: "text/css", body: css })
    return route.fulfill({ contentType: "text/html", headers: { "Content-Security-Policy": csp }, body: `<!doctype html>
      <link rel="stylesheet" href="xterm.css">
      <style nonce="terminal-test">body { font-family: sans-serif; color: white; background: black; }</style>
      <div id="terminal"></div><script src="xterm.js"></script>
      <script nonce="terminal-test">
        window.violations = [];
        document.addEventListener('securitypolicyviolation', event => window.violations.push(event.effectiveDirective));
        const terminal = new Terminal({ cols: 60, rows: 5, fontFamily: 'Consolas, monospace', fontSize: 13, theme: ${JSON.stringify(terminalTheme)} });
        terminal.open(document.getElementById('terminal'));
        terminal.write('\\x1b[31mRED\\x1b[0m \\x1b[38;2;12;160;220mRGB\\x1b[0m\\r\\nWWii', () => window.terminalReady = true);
      </script>` })
  })
  await page.goto("/terminal-csp/index.html")
  await page.waitForFunction(() => (window as unknown as { terminalReady: boolean }).terminalReady)
}

test("packaged terminal keeps ANSI colours and monospace cells without allowing inline scripts", async ({ page }) => {
  const disabled = security.dangerousDisableAssetCspModification
  // Do not solve a style-only compatibility issue by disabling script nonces.
  expect(disabled).not.toBe(true)
  expect(Array.isArray(disabled) && disabled.includes("script-src")).toBe(false)
  await renderTerminal(page, !(Array.isArray(disabled) && disabled.includes("style-src")))
  const red = page.locator(".xterm-rows span").filter({ hasText: /^RED$/ })
  await expect(red).toHaveCSS("color", "rgb(197, 15, 31)")
  await expect(red).toHaveCSS("font-family", "Consolas, monospace")
  await expect(red).toHaveCSS("display", "inline-block")
  await expect(page.locator(".xterm-rows span").filter({ hasText: /^RGB$/ })).toHaveCSS("color", "rgb(12, 160, 220)")
  expect(await page.evaluate(() => (window as unknown as { violations: string[] }).violations)).toEqual([])
  await page.evaluate(() => {
    const script = document.createElement("script")
    script.textContent = "window.untrustedScriptRan = true"
    document.body.appendChild(script)
  })
  await expect.poll(() => page.evaluate(() => (window as unknown as { violations: string[] }).violations)).toContain("script-src-elem")
  expect(await page.evaluate(() => (window as unknown as { untrustedScriptRan?: boolean }).untrustedScriptRan)).toBeUndefined()
})

test("release style nonce reproduces lost terminal colours and font", async ({ page }) => {
  await renderTerminal(page, true)
  const red = page.locator(".xterm-rows span").filter({ hasText: /^RED$/ })
  await expect(red).toHaveCSS("color", "rgb(255, 255, 255)")
  await expect(red).toHaveCSS("font-family", "sans-serif")
  await expect.poll(() => page.evaluate(() => (window as unknown as { violations: string[] }).violations)).toContain("style-src-elem")
})
