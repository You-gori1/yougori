export type EnvironmentAction = "duplicating" | "starting" | "opening" | "pausing" | "stopping" | "deleting" | "resetting" | "connecting" | "disconnecting"

export const environmentActionLabel: Record<EnvironmentAction, string> = {
  duplicating: "Duplicating…",
  connecting: "Connecting…",
  disconnecting: "Disconnecting…",
  starting: "Starting…",
  opening: "Opening…",
  pausing: "Pausing…",
  stopping: "Stopping…",
  deleting: "Deleting…",
  resetting: "Factory resetting…",
}
