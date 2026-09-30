import { invoke, isTauri } from "@tauri-apps/api/core"

export interface FileExportResult { file?: string; folder?: string; entries: number; bytes: number }
export interface EnvironmentTransferResult { source: string; sourceEnvironment: string; destination: string; files: number; bytes: number }

export const fileExportApi = {
  async chooseDestination() {
    if (!isTauri()) throw new Error("Copying files to this PC requires Yougori Desktop")
    const { open } = await import("@tauri-apps/plugin-dialog")
    const selected = await open({ directory: true, multiple: false, title: "Choose a folder on this PC" })
    return typeof selected === "string" ? selected : null
  },
  async copy(environmentId: string, path: string, destination: string) {
    if (!isTauri()) throw new Error("Copying files to this PC requires Yougori Desktop")
    return invoke<FileExportResult>("copy_files_from_environment", { environmentId, path, destination })
  },
  async between(sourceId: string, targetId: string, path: string) {
    if (!isTauri()) throw new Error("Copying between environments requires Yougori Desktop")
    return invoke<EnvironmentTransferResult>("copy_files_between_environments", { sourceId, targetId, path })
  },
}
