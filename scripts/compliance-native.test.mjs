import assert from "node:assert/strict"
import test from "node:test"
import { validateAngleNotices, validateAngleRecord } from "./compliance-native.mjs"

function fixture() {
  return {
    build: { schemaVersion: 1, profile: "angle-d3d11-only", revision: "pinned", binaries: [{ file: "libGLESv2.dll" }],
      inputs: [{ path: "src/common/third_party/xxhash/xxhash.c", sha256: "a".repeat(64), component: "xxhash" }],
      fontPreprocessing: { dataPresent: false, disabledBody: "return nullptr;" } },
    policy: { revision: "pinned", forbiddenSourcePrefixes: ["third_party/spirv-tools/", "third_party/vulkan_memory_allocator/"],
      interfaceHeaders: ["include/EGL/egl.h"],
      components: [{ id: "xxhash", license: "BSD-2-Clause", review: "approved", basis: "Exact upstream license", notice: "Copyright (c) 2012-2014, Yann Collet" }] },
  }
}

test("the native review covers embedded components even when the DLL has a permissive package label", () => {
  const { build, policy } = fixture()
  validateAngleRecord(build, policy)
  build.inputs.push({ path: "third_party/new-library/code.cc", sha256: "b".repeat(64), component: "unreviewed" })
  assert.throws(() => validateAngleRecord(build, policy), /Unreviewed embedded/)
})

test("an excluded implementation cannot be relabeled as an approved component", () => {
  const { build, policy } = fixture()
  for (const path of ["third_party/spirv-tools/source/libspirv.cpp", "third_party/vulkan_memory_allocator/include/vk_mem_alloc.h"]) {
    build.inputs[0].path = path
    assert.throws(() => validateAngleRecord(build, policy), /Excluded ANGLE implementation/)
  }
})

test("disabled font data and an actual component license decision are required", () => {
  const { build, policy } = fixture()
  build.fontPreprocessing.dataPresent = true
  assert.throws(() => validateAngleRecord(build, policy), /font exclusion/)
  build.fontPreprocessing.dataPresent = false
  policy.components[0].review = "pending"
  assert.throws(() => validateAngleRecord(build, policy), /Incomplete native/)
})

test("the Apache interface exception cannot cover a new implementation header", () => {
  const { build, policy } = fixture()
  policy.components[0].id = "khronos-apache-interfaces"
  build.inputs[0].component = "khronos-apache-interfaces"
  build.inputs[0].path = "include/EGL/egl.h"
  validateAngleRecord(build, policy)
  build.inputs[0].path = "third_party/implementation.h"
  assert.throws(() => validateAngleRecord(build, policy), /Unreviewed Apache interface/)
})

test("ANGLE's root BSD text does not substitute for its embedded xxHash notice", () => {
  const { policy } = fixture()
  assert.throws(() => validateAngleNotices("ANGLE BSD-3-Clause", policy.components), /notice missing: xxhash/)
  validateAngleNotices("ANGLE BSD-3-Clause\nCopyright (c) 2012-2014, Yann Collet", policy.components)
})
