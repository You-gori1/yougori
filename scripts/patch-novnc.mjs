// Remove OpenDock's former stretching extension from existing installations.
// Fresh installs already use unmodified noVNC. Keep this migration idempotent,
// version-pinned and fail-closed; never overwrite unrelated dependency changes.
import { readFile, writeFile } from "node:fs/promises"
const root = new URL("../node_modules/@novnc/novnc/", import.meta.url)
const pkg = JSON.parse(await readFile(new URL("package.json", root), "utf8"))
if (pkg.version !== "1.7.0") throw new Error("Review the Yougori viewport migration before upgrading noVNC")
const legacyPatches = {
  "core/display.js": [
    ["this._scale = 1.0;", "this._scale = 1.0;\n        this._scaleY = 1.0;"],
    ["    absY(y) {\n        if (this._scale === 0)", "    absY(y) {\n        if (this._scaleY === 0)"],
    ["y / this._scale + this._viewportLoc.y", "y / this._scaleY + this._viewportLoc.y"],
    ["autoscale(containerWidth, containerHeight) {", "autoscale(containerWidth, containerHeight, stretch = false) {\n        if (stretch && this._viewportLoc.w > 0 && this._viewportLoc.h > 0) {\n            this._rescale(containerWidth / this._viewportLoc.w, containerHeight / this._viewportLoc.h);\n            return;\n        }"],
    ["_rescale(factor) {\n        this._scale = factor;", "_rescale(factor, factorY = factor) {\n        this._scale = factor;\n        this._scaleY = factorY;"],
    ["const height = factor * vp.h + 'px';", "const height = factorY * vp.h + 'px';"],
  ],
  "core/rfb.js": [
    ["    get scaleViewport()", "    // OpenDock: edge-to-edge scaling, with per-axis pointer mapping.\n    get stretchViewport() { return Boolean(this._stretchViewport); }\n    set stretchViewport(value) {\n        this._stretchViewport = Boolean(value);\n        this._updateScale();\n    }\n\n    get scaleViewport()"],
    ["this._display.autoscale(size.w, size.h);", "this._display.autoscale(size.w, size.h, this._stretchViewport);"],
  ],
}
const marker = "// OpenDock viewport patch v1\n"
const changed = []
for (const [file, replacements] of Object.entries(legacyPatches)) {
  const path = new URL(file, root)
  let source = (await readFile(path, "utf8")).replaceAll("\r\n", "\n")
  if (!source.startsWith(marker)) {
    if (source.includes("_stretchViewport") || source.includes("this._scaleY")) throw new Error(`Unrecognized noVNC viewport modification in ${file}`)
    continue
  }
  source = source.slice(marker.length)
  for (const [before, after] of [...replacements].reverse()) {
    if (source.split(after).length !== 2) throw new Error(`Cannot safely remove legacy noVNC patch from ${file}: ${after}`)
    source = source.replace(after, before)
  }
  changed.push([path, source])
}
for (const [path, source] of changed) await writeFile(path, source)
