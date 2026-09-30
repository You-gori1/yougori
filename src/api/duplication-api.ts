import { run } from "@/api/platform-api"
import type { CloudDeployRequest } from "@/api/cloud-api"
import type { PlatformState } from "@/types/platform"

export interface CloudCopyLocation {
  provider: "aws" | "azure" | "google"
  account: string
  region: string
  instance: string
  resourceGroup: string
  bucket: string
}
export interface DuplicationRequest {
  operationId: string
  environmentId: string
  name: string
  destination: "local" | "cloud"
  source: CloudCopyLocation | null
  target: CloudDeployRequest | null
  targetBucket: string
  storageDrive: string | null
  reviewed: boolean
  localFiles?: { image: string; paths: string[]; storageGb: number } | null
}
export interface DuplicationJob {
  request: DuplicationRequest
  environmentId: string
  status: "running" | "failed" | "interrupted" | "complete"
  phase: string
  error: string | null
  resources: string[]
  completed?: Record<string, unknown>
}
export const duplicationApi = {
  cleanup: (operationId: string) => run<PlatformState>("cleanup_environment_duplication", { operationId }, () => { throw new Error("Cloud transfer cleanup requires Yougori Desktop") }),
  inspectSource: (environmentId: string, source: CloudCopyLocation) => run<{ ready: boolean; disk: string; bootMode: "uefi" | "legacy-bios" }>("inspect_duplication_source", { environmentId, source }, () => { throw new Error("Source VM inspection requires Yougori Desktop and the provider CLI") }),
  duplicate: (request: DuplicationRequest) => run<PlatformState>("duplicate_environment", { request }, () => { throw new Error("Cloud duplication requires Yougori Desktop and the provider CLI") }),
}
