import { readFileSync } from "node:fs"
import { resolve } from "node:path"
import { expect, test } from "vitest"

const read = (path: string) => readFileSync(resolve(process.cwd(), path), "utf8")

test("desktop and package branding use Yougori while migrating existing app data", () => {
  const config = JSON.parse(read("src-tauri/tauri.conf.json"))
  expect(config.productName).toBe("Yougori")
  expect(config.app.windows[0].title).toBe("Yougori")
  expect(config.identifier).toBe("com.yougori.desktop")
  expect(read("src-tauri/src/lib.rs")).toContain('migrate_legacy_app_data(&app_directory)')
  expect(read("src-tauri/src/workspace/cloudflare.rs")).toContain('const VAULT_SERVICE: &str = "Yougori.CloudflareTunnel.v1"')
  expect(read("src-tauri/src/backup.rs")).toContain('const CREDENTIAL_SERVICE: &str = "com.yougori.desktop.backup"')
  expect(JSON.parse(read("package.json")).name).toBe("yougori")
  expect(JSON.parse(read("package-lock.json")).name).toBe("yougori")
  expect(read("src-tauri/Cargo.toml")).toContain('name = "yougori"')
  expect(read("src-tauri/windows-app-manifest.xml")).toContain('name="Yougori"')
})

test("new backups and agent setup use the new name while old backups remain selectable", () => {
  expect(read("src-tauri/src/local_backup.rs")).toContain('const MANIFEST: &str = "backup.yougori"')
  expect(read("src/api/local-backup-api.ts")).toContain('extensions: ["yougori", "opendock"]')
  expect(read("skills/yougori/SKILL.txt")).toMatch(/name: yougori\r?\n/)
  expect(read("cli/src/skills.rs")).toContain('root.join("skills/yougori")')
  expect(read("scripts/bundle-cli.mjs")).toContain('"yougori-cli.exe"')
  expect(read("scripts/bundle-cli.mjs")).toContain('"opendock-cli.exe"')
})
