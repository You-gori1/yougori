import { Channel, invoke, isTauri } from "@tauri-apps/api/core"
import type { DragDropEvent } from "@tauri-apps/api/webview"

export interface FileCopyProgress {
  phase: "scanning" | "preparing" | "archiving" | "connecting" | "copying" | "sending" | "extracting" | "verifying" | "finishing"
  completedBytes: number
  totalBytes: number
  scannedEntries?: number
  transferId?: string
  sentBytes?: number
  confirmedBytes?: number
  lastProgressAt?: string
  waitingFor?: string
}
export interface FileCopyResult {
  destination: string
  files: number
  bytes: number
  skippedLinks: number
  /** Left out by the copied folder's .yougoriignore. */
  skippedIgnored?: number
  delivery: "directory" | "drive"
}
export interface ImportedDrive { id: string; attached: boolean; bytes: number }

export const fileImportApi = {
  async cancel(environmentId: string, transferId?: string): Promise<{ cancelRequested: boolean }> {
    if (!isTauri()) throw new Error("File transfer cancellation requires the Yougori desktop app.")
    return invoke("cancel_file_transfer", { environmentId, transferId })
  },
  async drives(environmentId: string): Promise<ImportedDrive[]> {
    if (!isTauri()) return []
    return invoke("list_imported_drives", { environmentId })
  },
  async setDriveAttached(environmentId: string, transferId: string, attached: boolean): Promise<ImportedDrive[]> {
    if (!isTauri()) throw new Error("Imported drives require the Yougori desktop app.")
    return invoke("set_imported_drive_attached", { environmentId, transferId, attached })
  },
  async listen(callback: (event: DragDropEvent) => void) {
    if (!isTauri()) return () => undefined
    const { getCurrentWebview } = await import("@tauri-apps/api/webview")
    return getCurrentWebview().onDragDropEvent(event => callback(event.payload))
  },
  async copy(environmentId: string, paths: string[], progress: (event: FileCopyProgress) => void) {
    if (!isTauri()) throw new Error("Drop files in the Yougori desktop app to copy them into an environment.")
    const onProgress = new Channel<FileCopyProgress>()
    onProgress.onmessage = progress
    return invoke<FileCopyResult>("copy_files_to_environment", { environmentId, paths, onProgress })
  },
}
