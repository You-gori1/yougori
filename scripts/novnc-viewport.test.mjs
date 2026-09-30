import { test } from "node:test"
import assert from "node:assert/strict"
globalThis.window = { console }
const { default: Display } = await import("../node_modules/@novnc/novnc/core/display.js")

const canvas = () => ({ width: 1024, height: 768, style: {}, getContext: () => ({}) })
globalThis.document = { createElement: canvas }

test("wide windows never stretch the desktop or its pointer coordinates", () => {
  const target = canvas(), display = new Display(target)
  display.autoscale(1600, 900)
  assert.equal(target.style.width, "1200px")
  assert.equal(target.style.height, "900px")
  assert.equal(display.absX(600), 512)
  assert.equal(display.absY(450), 384)
  assert.equal(display.absX(1199), 1023)
  assert.equal(display.absY(899), 767)
})
test("portrait windows preserve proportions and the whole framebuffer", () => {
  const target = canvas(), display = new Display(target)
  display.autoscale(600, 1000)
  assert.equal(target.style.width, "600px")
  assert.equal(target.style.height, "450px")
  assert.equal(display.absX(300), 512)
  assert.equal(display.absY(225), 384)
})
test("a matching guest resolution fills both dimensions without stretching", () => {
  const target = canvas(), display = new Display(target)
  display.autoscale(1200, 900)
  assert.equal(target.style.width, "1200px")
  assert.equal(target.style.height, "900px")
  assert.equal(display.absX(600), 512)
  assert.equal(display.absY(450), 384)
  display.scale = 1
  assert.equal(display.absY(500), 500)
})
test("hidden views do not produce infinite pointer coordinates", () => {
  const display = new Display(canvas())
  display.autoscale(0, 0)
  assert.equal(display.absX(20), 0)
  assert.equal(display.absY(20), 0)
})
