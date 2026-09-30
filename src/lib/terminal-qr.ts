import qrcode from "qrcode-generator"

export function createTerminalQr(url: string): { size: number; path: string } | null {
  try {
    const bytes = new TextEncoder().encode(url)
    if (bytes.length > 2048) return null
    const code = qrcode(0, "M")
    // The generator's default Byte mode is Latin-1; provide UTF-8 bytes.
    code.addData(String.fromCharCode(...bytes), "Byte")
    code.make()
    const size = code.getModuleCount()
    let path = ""
    for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) if (code.isDark(y, x)) path += `M${x + 4} ${y + 4}h1v1h-1z`
    // Four white modules on every side are the QR quiet zone.
    return { size: size + 8, path }
  } catch { return null }
}
