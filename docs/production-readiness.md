# Production preparation

Feature freeze: stabilize the existing desktop app and CLI. Fix defects, packaging,
installation, updates and reliability before adding more features.

## Initial release targets

| Operating system | Processor | Desktop package | Standalone CLI + engine |
| --- | --- | --- | --- |
| Windows | Intel/AMD x64 | NSIS installer and MSI | ZIP |
| macOS 14+ | Apple Silicon arm64 | DMG | tar.gz |
| macOS 14+ | Intel x64 | DMG | tar.gz |
| Debian/Ubuntu Linux | Intel/AMD x64 | DEB | tar.gz |
| 64-bit Debian/Ubuntu/Raspberry Pi OS | ARM64 | Not published | tar.gz |

These are candidate targets, not a statement that all platforms have passed
release validation. Linux CI uses Ubuntu 22.04. Other Linux distributions and
Windows ARM64 requires separate runtime and installer work before support
can be advertised. Linux ARM64 and Apple Silicon run the x86-64 guest appliance
through software emulation; use amd64 guest images. ARM-native guests and GPU/CUDA
are unavailable. Physical Raspberry Pi runtime acceptance remains required.

The Linux ARM64 CLI and engine can be cross-built on Linux with an ARM64 GCC
linker and an ARM64 GTK3 development sysroot. Select both the payload architecture
and Cargo target explicitly, then verify the extracted binaries on ARM64:

```sh
export YOUGORI_RELEASE_ARCH=arm64
export CARGO_BUILD_TARGET=aarch64-unknown-linux-gnu
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_SYSROOT_DIR=/path/to/arm64-sysroot
export PKG_CONFIG_LIBDIR="$PKG_CONFIG_SYSROOT_DIR/usr/lib/aarch64-linux-gnu/pkgconfig:$PKG_CONFIG_SYSROOT_DIR/usr/share/pkgconfig"
npm run cli:bundle
npm run engine:package -- --preview
```

Do this in an isolated release checkout so its bundled CLI does not replace the
developer's host CLI. The installer selects `linux-aarch64-engine` on 64-bit Pi OS;
32-bit ARM is not supported. QEMU and GTK3 remain system dependencies.

## Build candidates

Build locally without GitHub Actions or a commit:

```powershell
npm run release:local -- --output D:\Yougori-Releases\candidate --runtime-tests
```

Use a new output directory. The command captures current source, including new
files, in an isolated checkout with its own Git index. It keeps the developer
index and live runtime unchanged. Locked dependencies, audits, lint, unit,
browser, compliance and native Rust checks run before desktop and standalone
engine packaging. On Windows, `--runtime-tests` also runs the disposable native
smoke gate. Packages, matching source delivery, SHA-256 checksums, logs and a
machine-readable acceptance report stay together in the output directory.

Run the same command natively on Linux or macOS without `--runtime-tests`, then
run their platform runtime/installation checks. This requires an existing complete
`build/compliance/bundle` and collector records; missing or mismatched sources
fail the build. Windows builds need the Visual Studio C++ toolchain, Python,
PowerShell 7 and Git Bash. Linux needs the native Tauri build dependencies.
No CI, commit, publishing or replacement of the installed app is involved.
These builds are unsigned candidates for validation. The report does not grant
release approval or infer clean-machine installation from a successful build.

The source preparation step verifies archives against the existing reviewed
records; it does not approve changed source or runtime bytes. A stale inventory
must fail the build. Follow [release validation](release-validation.txt) to prepare
an isolated checkout and review matching source material. Never rebuild or
replace the backing appliance of a running development environment.

## Current local evidence (2026-09-30)

- Local isolated candidates built Windows NSIS/MSI, Linux DEB, standalone engine
  archives and matching source ZIPs without commits or GitHub Actions. Packages,
  exact checksums, logs and reports are in `D:\Yougori-Releases\2026-09-30-local`.
- Windows passed 155 script tests, 505 frontend tests, all 190 browser tests,
  299 desktop library tests, CLI/CUDA/vault tests and desktop/engine checks.
  All 14 disposable native smoke tests passed. CLI lifecycle, snapshot/access
  enforcement, storage reclamation and 36 GB live memory growth passed; the
  memory test passed after concurrent builds released host memory.
- Ubuntu 22.04 passed 135 script tests (20 platform skips), 505 frontend tests,
  289 desktop library tests, CLI/CUDA/vault tests and desktop/engine checks.
  The browser suite passed 187 tests with two platform skips; one timing failure
  was corrected and both affected regressions passed. Go vet/race tests,
  Python cloud-agent tests and Rust/npm dependency audits passed. Rust reports
  six unmaintained dependency warnings; these are retained in the audit logs.
- Extracted Windows installers matched all 495 configured resources, including
  285 runtime files. Desktop and engine CLI aliases execute and match their
  packaged source binaries. License/notice checks passed for both installers
  and standalone archives. Linux packages matched all 85 configured resources,
  declare native QEMU dependencies and contain no Windows binaries.
- The packaged Linux standalone engine started without a display or DBus in
  new XDG directories, answered the packaged CLI and shut down cleanly.
- Runtime checksums were verified against the existing records. Source material
  and preview compliance gates verified 239 matching archives. These candidates
  remain unsigned and `productionReady` is false; clean-machine install/upgrade,
  native macOS, signing, engineering review and
  account-backed acceptance are still required.
- Exact Windows and Linux candidate source ZIPs are now published and verified
  on the public `you-gori/yougori` repository. This completes public source
  delivery for those recorded candidate hashes; it does not approve later source
  edits or establish trusted signatures or clean-machine acceptance.

## Production release follow-up (2026-09-30)

The signing preflight still fails: neither Windows certificate store has a
code-signing certificate and no signing service is configured. Windows Sandbox
is not installed, and no native Mac test/build host is configured. These are
actual outstanding prerequisites, not release labels to remove.

The Windows signing helpers now load their own interpreter's security module,
including when launched from PowerShell 7. The download manifest generator checks
trusted signatures and timestamps, handles paths containing spaces, pins the
publisher, and writes the production manifest after preparing the install files.
The website has a local production importer that checks exact package hashes,
Windows desktop/engine signatures, corresponding-source URLs and platform
installation evidence before enabling ordinary install commands. It retains
the unsigned preview flow until those checks pass.

## Previous evidence (2026-09-29)

- Windows: lint, production frontend build, 135 script tests, 505 frontend tests, CLI/CUDA/vault
  Rust tests, 297 desktop library tests, desktop all-targets check and engine
  check passed. All 189 browser tests passed across the full run and a focused
  rerun of four timeouts. npm audit reported zero vulnerabilities on retry.
- The disposable Windows storage test reclaimed 273,809,408 bytes, preserved
  a peer container's data and completed without compaction warnings. The real
  Codex installer smoke test passed in a fresh Ubuntu 24.04 container.
- WSL Ubuntu 22.04: Go vet/race tests, Python cloud-agent tests, lint, frontend
  build, script tests (118 passed, 20 platform skips), 505 frontend tests, compliance tests, CLI/vault tests,
  287 desktop library tests and desktop/engine checks passed.
- The Linux storage test passed on retry after a registry DNS timeout, reclaimed
  268,566,528 bytes and preserved the peer container's data.
- Both macOS architectures passed CLI, vault, engine and desktop all-targets
  type-checks from WSL. These used a stub C compiler without Apple's SDK;
  they do not establish native linking, packaging or runtime behavior.
- Windows MSI/NSIS, Linux DEB and standalone engine archives on both systems
  passed the official preview packaging commands, including their compliance
  gates. The DEB declares `qemu-system-x86` and contains no `.exe` or `.dll` files.
  These artifacts are unsigned previews, not production release approval.
- Current runtime payload hashes match their existing records. Regenerated
  application evidence and preview checks verified 239 source archives with no
  unresolved material items. Engineering review remains pending. Launcher edits
  were staged by their owning session and checked again; ongoing changes require
  new evidence and validation.
- Local verification records and artifacts are retained under
  `artifacts/production-readiness-2026-09-29/`. Native clean-machine installation,
  upgrade and macOS runtime validation remain outstanding.

## Open release blockers

1. **Release-specific review:** source collection and preview checks now pass,
   but engineering review remains pending. Review the complete application,
   exact final artifacts and matching source delivery in an isolated candidate
   checkout. Rerun affected checks and regenerate evidence after further edits.
   Do not approve a release merely because material checks pass, or accept
   changed runtime bytes just by replacing a checksum.
2. **Signing:** the maintainer does not yet have a Windows code-signing certificate
   or Apple Developer account. Obtain these before public signed builds. Use
   [Windows signing instructions](release-validation.txt) and the
   [macOS checklist](macos.md). Keep credentials outside Git.
3. **Native validation:** run all four native candidate builds and test the actual
   packages on clean machines. A passing Windows run does not establish Mac/Linux
   support. Verify desktop and standalone engine installations separately.
4. **Lifecycle and updates:** validate first install, upgrade from a real older
   version, reinstall, restart, uninstall with retained data, CLI PATH and recovery
   after interruption. Record exact package hashes and test results.
5. **Model and Neocloud acceptance:** verify local and RunPod model chat, API key
   authentication, public API link readiness, new chat, GPU count/availability,
   and repeated Ctrl+C with stop/delete on disposable resources. Account-backed
   tests need a defined spend limit and must record cleanup of the test pod.
6. **Publication:** publish matching source archives, verify the downloads, then
   run `npm run release:distribution`. Generate the download manifest only for
   packages that passed platform, signature, source and installation checks.

## Release acceptance record

For each platform retain: exact source manifest, base commit, toolchain versions,
artifact SHA-256, local build report, clean-machine OS version,
install/upgrade/lifecycle results, signing and
notarization results where applicable, known limitations, and rollback instructions.
Use the exact tested artifacts for release. Rebuilding invalidates their hashes
and requires package/signature verification again.

See [build cache management](build-cache.md) for local disk usage. Incremental
compilation is disabled. CI does not retain Cargo compiler caches; downloadable
candidate artifacts have a bounded retention period.
