import { readFile, writeFile } from "node:fs/promises"
import { fileURLToPath } from "node:url"
import { execFileSync } from "node:child_process"
import { chromium } from "@playwright/test"

const root = new URL("../", import.meta.url)
const source = await readFile(new URL("logo1.png", root))
const browser = await chromium.launch({ channel: "chromium" })
let icon
try {
  const page = await browser.newPage()
  icon = await page.evaluate(async data => {
    const image = new Image()
    image.src = `data:image/png;base64,${data}`
    await image.decode()
    // Preserve the complete artwork, including the rounded tile's outer edges.
    const size = Math.max(image.naturalWidth, image.naturalHeight)
    const canvas = document.createElement("canvas")
    canvas.width = size
    canvas.height = size
    canvas.getContext("2d").drawImage(
      image,
      Math.floor((size - image.naturalWidth) / 2),
      Math.floor((size - image.naturalHeight) / 2),
    )
    return canvas.toDataURL("image/png").split(",")[1]
  }, source.toString("base64"))
} finally {
  await browser.close()
}

const iconSource = new URL("src-tauri/icons/icon-source.png", root)
await writeFile(iconSource, Buffer.from(icon, "base64"))
execFileSync(process.execPath, [
  fileURLToPath(new URL("node_modules/@tauri-apps/cli/tauri.js", root)),
  "icon", fileURLToPath(iconSource), "-o", fileURLToPath(new URL("src-tauri/icons/", root)),
], { cwd: fileURLToPath(root), stdio: "inherit" })
