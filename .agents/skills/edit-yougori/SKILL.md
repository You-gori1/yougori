---
name: edit-yougori
description: Edit and test the Yougori desktop app in its source checkout. Use for changes to the React UI, Tauri backend, CLI, appliance, vault, or bundled runtime.
---

# Edit Yougori

Work from this checkout's root. Inspect the relevant code and current `git status` before editing; this checkout may contain unrelated user changes. Keep those changes intact.

- Desktop UI: `src/` (React and TypeScript). Tauri commands and runtime: `src-tauri/src/`. Public CLI: `cli/src/`. Vault helper: `vault/src/`. Guest appliance: `appliance/`.
- For a UI change, run the focused test when one exists, then `npm run build`. For Rust changes, run the relevant Cargo test and `cargo check --locked --manifest-path src-tauri/Cargo.toml --all-targets`. Use narrower checks when the change only touches CLI or vault.
- `npm run desktop:dev` runs this source checkout. An installed Yougori app is built from packaged files; editing this checkout does not change that installed binary until it is rebuilt and installed.
- Report the changed files, checks run, and any remaining limitation. Do not claim the app was rebuilt or installed unless that happened.

The Yougori management skill in `~/Yougori/Workspace/skills/yougori/` covers operating environments through the CLI. This skill covers changing the application source.
