"""Regression checks for source-level native license classification."""
from pathlib import Path
import runpy
import unittest

M = runpy.run_path(str(Path(__file__).with_name("compliance-angle.py")))


class NativeSourceTests(unittest.TestCase):
    def test_only_reachable_targets_are_built(self):
        graph = {"egl": {"deps": ["gles"]}, "gles": {"deps": ["xxhash"]}, "xxhash": {}, "unused-vulkan": {}}
        self.assertEqual(M["closure"](graph, ["egl"]), ["egl", "gles", "xxhash"])

    def test_vma_and_spirv_cannot_hide_under_angle(self):
        for name in ("third_party/vulkan_memory_allocator/include/vk_mem_alloc.h", "third_party/spirv-tools/source/libspirv.cpp"):
            with self.assertRaisesRegex(ValueError, "Excluded"):
                M["component_for"](name, "BSD-3-Clause")

    def test_new_implementation_in_interface_header_requires_review(self):
        name = "include/EGL/egl.h"
        self.assertEqual(M["component_for"](name, "/* Apache-2.0 */\nextern int eglInitialize(void *);"), "khronos-apache-interfaces")
        with self.assertRaisesRegex(ValueError, "contains implementation"):
            M["component_for"](name, "inline int function() { return 1; }")

    def test_bison_skeleton_exception_must_survive(self):
        with self.assertRaisesRegex(ValueError, "Bison output exception"):
            M["component_for"]("src/compiler/translator/glslang_tab_autogen.cpp", "GNU General Public License")

    def test_new_license_inside_angle_requires_review(self):
        with self.assertRaisesRegex(ValueError, "Unreviewed native license"):
            M["component_for"]("src/new_component.cpp", "// SPDX-License-Identifier: Apache-2.0")


if __name__ == "__main__":
    unittest.main()
