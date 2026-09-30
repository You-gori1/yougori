import type { ITheme } from "@xterm/xterm"

// Windows Terminal's familiar Campbell palette. True-colour/256-colour escape
// sequences remain controlled by each shell or program, with no text rewriting.
export const terminalTheme: ITheme = {
  background: "#0c0c0c", foreground: "#cccccc", cursor: "#ffffff",
  selectionBackground: "#264f78",
  black: "#0c0c0c", red: "#c50f1f", green: "#13a10e", yellow: "#c19c00",
  blue: "#0037da", magenta: "#881798", cyan: "#3a96dd", white: "#cccccc",
  brightBlack: "#767676", brightRed: "#e74856", brightGreen: "#16c60c",
  brightYellow: "#f9f1a5", brightBlue: "#3b78ff", brightMagenta: "#b4009e",
  brightCyan: "#61d6d6", brightWhite: "#f2f2f2",
}
