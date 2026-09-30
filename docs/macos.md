# macOS release checklist

Build and test separately on native Intel and Apple Silicon Macs running macOS
14 or newer. Use native Node, Rust and Homebrew. Apple Silicon x86-64 guests use
software emulation; local GPU/CUDA guest support is not provided by this preview.

## Development and candidate builds

1. Install Xcode command-line tools, Node and Rust.
2. Run `npm run macos:setup` to install/check Homebrew QEMU and cloudflared.
3. Prepare the reviewed source records described in
   [release validation](release-validation.txt).
4. Run `npm run macos:build`, then `npm run engine:package`.
5. Run `npm run macos:test:runtime` on disposable workloads.

The build checks bundle architecture and layout. It also verifies source archives
against the reviewed release records. It does not establish public signing or
notarization. Keep QEMU/cloudflared as documented runtime prerequisites until a
self-contained runtime has separately been built and validated.

## Public distribution

An Apple Developer account and a Developer ID Application signing identity are
required for the planned direct distribution. Sign all shipped executable code,
including the CLI and standalone engine, enable hardened runtime, notarize, and
validate Gatekeeper acceptance. Follow the current
[Tauri signing guide](https://tauri.app/distribute/sign/macos/) and
[Apple notarization guide](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution).
Do not tell users to disable Gatekeeper or remove quarantine as an installation step.

On each architecture, use a clean account and test:

- Downloaded DMG mounting, drag-to-Applications install, first launch, CLI linking
  and permission prompts under normal Gatekeeper settings.
- Standalone CLI + engine installation independently of the desktop app, including
  discovery of external runtime dependencies and operation without an open window.
- Container start/stop, terminal input/resize, sharing, microVM/full VM behavior,
  network access and complete owned-process shutdown.
- Model chat and API access, with Neocloud tests tracked separately when using a
  paid provider account.
- Upgrade, reinstall, login startup, uninstall and preservation of user data.

Record the package SHA-256, macOS version, architecture and results in the release
acceptance record. Unsigned CI artifacts cannot satisfy the distribution checks.
