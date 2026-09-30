import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import test from "node:test"

test("Vite excludes generated native/build/test trees from its file watcher", () => {
  const source = readFileSync(new URL("../vite.config.ts", import.meta.url), "utf8")
  const ignored = source.match(/watch:\s*\{\s*ignored:\s*(\[[^\]]+\])/)
  assert.ok(ignored, "Keep explicit watcher exclusions for generated trees")
  const patterns = JSON.parse(ignored[1])
  for (const directory of ["src-tauri", "target", "build", "artifacts", "test-results*", "playwright-report"]) {
    assert.ok(patterns.includes(`**/${directory}/**`), `Do not watch generated ${directory} files`)
  }
  assert.ok(!patterns.includes("**/src/**"), "Frontend source must remain watched")
})
