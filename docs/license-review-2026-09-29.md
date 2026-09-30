# License review and repairs, 2026-09-29

Original Yougori code can retain its current AGPL-3.0-only declaration. This review found repairable notice, source-evidence and release-tooling gaps, rather than a demonstrated requirement to replace that license. The repository repairs below are complete. Binary release approval remains pending for the specific release checks listed at the end.

## Scope

The audit inventoried all 1,377 files tracked when it began, including text and binary material. It scanned text for license declarations, copyright and copied-code markers, and inspected the applicable root/component notices, dependency manifests, build scripts, resource maps and source collectors. Subsequent audit files and the user's concurrent README/image addition are included in refreshed source coverage. This was a repository-wide licensing review, not a line-by-line functional or security review of every implementation.

Coverage includes the React UI, desktop Rust backend, standalone engine, CLI, vault, CUDA host code, Go appliance and cloud-share helper, Python model/cloud/vault scripts, C/EFI helpers, vendored glib, copied Coss UI, assets, installers, bundled native libraries, firmware, guest packages and historical compliance records. The complete file list and hashes are in `compliance/evidence/application-source.json` and `compliance/evidence/runtime-files.json`; these are checked against the working tree, not inferred from a list of directory names.

## Repairs completed

1. **All Rust dependency graphs are discovered.** The collector no longer maintains a fixed list that omitted the engine. It discovers all seven Git-visible Cargo lockfiles, including vendored builds. Unknown registry/Git sources and conflicting package checksums require explicit handling.
2. **Notices refreshed.** The application inventory contains 907 records: 859 registry crate versions, 46 npm package versions, one copied UI component set and one vendored Rust package. Every record has notice text. The 48 missing engine package/version pairs and seven missing app/CLI pairs identified earlier are covered, along with additional vendored-build entries. Counts include build-only and other-target dependencies.
3. **Independent dependency checks added.** Ordinary compliance checks compare current lockfiles with exact checksums/integrities and notice records. Rust checks also verify complete rendered license sections in the shipped notice file, so package names alone cannot stand in for attribution.
4. **Frontend import scanning fixed.** JavaScript/TypeScript imports are parsed as code. Displayed examples such as `import OpenAI from "openai"` no longer become false dependencies. Real lazy imports, re-exports, require calls and executable template expressions remain covered.
5. **Complete source checking expanded.** Ordinary checks now compare the entire application source manifest, including backend and CLI files. Newly added untracked assets cannot be silently omitted from source delivery.
6. **Runtime evidence refreshed.** Inventoried 60 Alpine packages, 70 Windows DLLs and 244 initramfs entries. Guest Go dependency sources were recollected. Runtime notice coverage is 209 of 209 recorded source/package entries. The assembled source index has 239 archives.
7. **Exact QEMU source recovered.** A branding edit had changed the development TPM backend after the bundled binaries were built. `runtime/security/shipped/tpm-qemu.c` preserves the original GPL-covered source, recovered from Git and matched to the recorded SHA-256. Rebuild instructions now use it for the existing binaries. All 12 functional source hashes and ten dated upstream modification notices verified. The newer development source is preserved.
8. **Native linkage rechecked.** Re-inspected 77 Windows PE files and compared their imports, hashes and retained native review. The forbidden JACK/Berkeley DB/TPM/OpenSSL linkage did not reappear in QEMU. Existing ANGLE source and notice checks pass.
9. **Documentation repaired.** Corrected the root licensing-guide link, restored the missing compliance-status document and explained retained QEMU sources and source-manifest identity.
10. **Release metadata strengthened.** Website release-manifest generation now runs the distribution gate, requires verified matching-source URL/checksum metadata and a clean committed checkout, and includes a source link and exact commit beside binary asset metadata. Standalone engine packaging checks its staged license files against the reviewed resource configuration.
11. **Source preparation separated from binary approval.** A complete, checksum-verified source ZIP can be prepared for review without declaring installers approved. Missing source collection items still block creation of that complete bundle.
12. **Approval reuse prevented.** A source fingerprint binds an engineering approval to the collected application, runtime and archives. Regenerating changed evidence invalidates an older approval.
13. **Build prerequisites made explicit.** Compliance CI uses pinned Python 3.12; local dependency checking reports its Python 3.11 minimum.
14. **Upstream texts preserved.** Added whitespace attributes for generated notice files so intact upstream license text is not rewritten merely to satisfy whitespace lint.

The root grant, third-party exceptions and commercial-licensing scope were not replaced. The user's concurrent README/image edits were preserved. No app UI redesign, screenshot capture, running-environment change, installation, commit or public push was performed.

## License assessment

The app, CLI, engine, vault and CUDA manifests consistently declare AGPL-3.0-only. The commercial option remains limited to rights Yougori owns or is authorized to grant. Contributor assignments and private commercial agreements are not present in this review's evidence and have not been certified.

The collected application expressions offer permissive options or MPL terms with retained source. `self_cell` offers Apache-2.0 as an alternative to GPL-2.0-only; `r-efi` offers MIT/Apache alternatives to LGPL. These alternatives must not be mistaken for mandatory GPLv2-only linkage. The new `notify` dependency carries CC0-1.0; its full dedication and fallback text is retained. No missing-license or unknown-license declaration was left in the collected application inventory. Actual native combinations continue to use their separately recorded component review.

Hugging Face model weights, downloaded PyTorch/CUDA and Python images, user workloads and third-party cloud services retain their own terms. Yougori's license does not grant rights to every model or cloud service a user selects. No blanket clearance of arbitrary downloaded workloads is claimed. Bundled QEMU and its GPL-covered changes retain their own license; the commercial option does not waive source, notice or applicable replacement/relinking requirements.

## Verification

- `npm run test:compliance`: passing, including new engine-discovery, missing-attribution, whole-source, untracked-asset and retained-build-source regressions.
- `node --test scripts/release-manifest.test.mjs`: passing.
- `node scripts/compliance-check.mjs`: passing with zero source-collection blockers.
- `node scripts/compliance-check.mjs --archives`: passing for 239 source archives and the complete application archive contents.
- QEMU source restoration/annotation checks: all 12 original functional source hashes match.
- All three EFI helpers: rebuilt with the repository's `-Check` commands and matched the shipped binaries exactly.
- Guest agent: compiled from current source with Go 1.27.1 and the recorded Go 1.26.8. The latter builds successfully but is not byte-identical to the existing binary: rebuilt SHA-256 `13bfd9d383f6d59bffd3818e01f439f9db83ff86ef8a36730efe5c1cfe9ae58a`, bundled SHA-256 `a983fefd176a3145826012a6aab366a6b0d714222fa2a8226ca6b197e96abf6a`. The source tree matches the tracked source at the binary's commit, and embedded dependency versions/checksums were collected. This establishes buildability and source availability, not reproducibility; no bundled agent was replaced.

Local execution logs and rebuild outputs are retained under `build/compliance/audit-2026-09-29/`. Source archives are in `build/compliance/bundle/`; a review ZIP is prepared under `build/compliance/dist/`.

## Remaining release-specific work

The engineering review remains pending. Zero automated collection blockers is not legal certification or approval of uninspected installers.

- Establish the guest-agent build difference before claiming reproducibility, or produce and test a new recorded payload from the collected source.
- Build the intended desktop and standalone engine artifacts and inspect their actual extracted notices and payloads. This review checked configurations, staged-engine enforcement and existing native payloads; it did not build all platform installers.
- Publish and verify the matching current source ZIP beside those exact binary downloads. The public repository currently lists two September 14 source-only prereleases, not a source publication for this candidate. Nothing was uploaded by this task.
- Resolve the separate older-installer records in `compliance/history` and `compliance/evidence/website-installers.json`. Those records identify actual September 11/12 distributions whose complete coverage is still pending. New notices cannot retroactively establish their compliance.
- Confirm ownership/assignment authority for any proposed commercial agreement. Repository license text alone is not proof of signed assignments.

These items cannot honestly be marked solved merely by changing an approval flag or substituting current source for an older binary. This is an engineering assessment, not a legal opinion.

References: [GNU licensing FAQ](https://www.gnu.org/licenses/gpl-faq.en.html), [GNU license compatibility](https://www.gnu.org/licenses/license-compatibility.en.html), [QEMU licensing](https://www.qemu.org/docs/master/about/license.html), [Mozilla MPL FAQ](https://www.mozilla.org/en-US/MPL/2.0/FAQ/), [Hugging Face license documentation](https://huggingface.co/docs/hub/repositories-licenses), and the complete AGPL text in `COPYING`.
