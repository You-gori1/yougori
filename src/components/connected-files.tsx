import { useEffect, useMemo, useRef, useState } from "react"
import { ChevronLeftIcon, DownloadIcon, FileIcon, FolderIcon, RefreshCwIcon, SaveIcon, UploadIcon } from "lucide-react"
import { platformApi } from "@/api/platform-api"
import { Button } from "@/components/ui/button"
import { usePlatform } from "@/context/platform-context"

type Entry = { name: string; directory: boolean; size: number }
type OpenFile = { name: string; path: string; original: string; text: string }
const chunk = 64 * 1024

function fromBase64(value: string) { return Uint8Array.from(atob(value), character => character.charCodeAt(0)) }
function toBase64(value: Uint8Array) {
  let binary = ""
  for (let offset = 0; offset < value.length; offset += 8192) binary += String.fromCharCode(...value.subarray(offset, offset + 8192))
  return btoa(binary)
}

export function ConnectedFiles({ environmentId, active }: { environmentId: string; active: boolean }) {
  const { state } = usePlatform()
  const choices = useMemo(() => (state?.connections ?? []).filter(connection => connection.active
    && (connection.sourceId === environmentId || connection.targetId === environmentId)
    && connection.permissions.some(permission => ["data", "files", "volumes"].includes(permission))).flatMap(connection => {
      const peerId = connection.sourceId === environmentId ? connection.targetId : connection.sourceId
      const peer = state?.environments.find(environment => environment.id === peerId)
      const writable = connection.sourceId === environmentId || connection.direction === "bidirectional"
      const selected = (connection.selectedFolders ?? []).map((folder, index) => ({
        key: `${connection.id}:${index}`, connectionId: connection.id, root: `_selected/${index}`,
        label: `${state?.environments.find(environment => environment.id === folder.environmentId)?.name ?? "Environment"} · ${folder.path}`,
        writable,
      }))
      return [...selected, { key: `${connection.id}:base`, connectionId: connection.id, root: "", label: `${peer?.name ?? "Environment"} · connection folder`, writable }]
    }), [environmentId, state?.connections, state?.environments])
  const [selectedKey, setSelectedKey] = useState("")
  const choice = choices.find(item => item.key === selectedKey) ?? choices[0]
  const [parts, setParts] = useState<string[]>([])
  const [entries, setEntries] = useState<Entry[]>([])
  const [openFile, setOpenFile] = useState<OpenFile | null>(null)
  const [loading, setLoading] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")
  const [revision, setRevision] = useState(0)
  const lock = useRef(false)
  const path = [choice?.root, ...parts].filter(Boolean).join("/")
  const connectionId = choice?.connectionId
  const request = (operation: string, filePath: string, extra: Record<string, unknown> = {}) => {
    if (!choice) throw new Error("Choose a shared folder")
    return platformApi.requestConnectedFiles(environmentId, { connectionId: choice.connectionId, operation, path: filePath, ...extra })
  }
  useEffect(() => {
    if (!active || !connectionId) return
    let cancelled = false
    setLoading(true)
    setError("")
    void platformApi.requestConnectedFiles(environmentId, { connectionId, operation: "list", path })
      .then(result => { if (!cancelled) setEntries(result.entries ?? []) })
      .catch(reason => { if (!cancelled) { setEntries([]); setError(reason instanceof Error ? reason.message : String(reason)) } })
      .finally(() => { if (!cancelled) setLoading(false) })
    return () => { cancelled = true }
  }, [active, environmentId, connectionId, path, revision])
  const perform = async (action: () => Promise<void>) => {
    if (lock.current) return
    lock.current = true; setBusy(true); setError("")
    try { await action() } catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)) }
    finally { lock.current = false; setBusy(false) }
  }
  const choose = (key: string) => { setSelectedKey(key); setParts([]); setEntries([]); setOpenFile(null) }
  const open = (entry: Entry) => void perform(async () => {
    if (entry.size > chunk) throw new Error("Text editing is limited to 64 KB. Download this file to open it elsewhere.")
    const filePath = `${path}/${entry.name}`
    const result = await request("read", filePath, { length: Math.max(1, entry.size) })
    const bytes = fromBase64(result.data ?? "")
    if (bytes.includes(0)) throw new Error("This is a binary file. Download it instead.")
    setOpenFile({ name: entry.name, path: filePath, original: result.data ?? "", text: new TextDecoder("utf-8", { fatal: true }).decode(bytes) })
  })
  const save = () => void perform(async () => {
    if (!openFile || !choice?.writable) return
    const data = new TextEncoder().encode(openFile.text)
    if (data.length > chunk) throw new Error("Text editing is limited to 64 KB")
    const encoded = toBase64(data)
    await request("replace", openFile.path, { expectedData: openFile.original, data: encoded })
    setOpenFile({ ...openFile, original: encoded })
    setRevision(value => value + 1)
  })
  const download = (entry: Entry) => void perform(async () => {
    if (entry.size > 128 * 1024 * 1024) throw new Error("Downloads are limited to 128 MB")
    const pieces: BlobPart[] = []
    const filePath = `${path}/${entry.name}`
    for (let offset = 0; offset < entry.size; offset += chunk) {
      const result = await request("read", filePath, { offset, length: Math.min(chunk, entry.size - offset) })
      const data = fromBase64(result.data ?? "")
      const buffer = new ArrayBuffer(data.length)
      new Uint8Array(buffer).set(data)
      pieces.push(buffer)
      if (data.length !== Math.min(chunk, entry.size - offset)) throw new Error("The download stopped before the file was complete. Try again.")
    }
    const blob = new Blob(pieces)
    const url = URL.createObjectURL(blob)
    const anchor = document.createElement("a")
    anchor.href = url; anchor.download = entry.name; anchor.click()
    setTimeout(() => URL.revokeObjectURL(url), 30000)
  })
  const upload = (file: File) => void perform(async () => {
    if (!choice?.writable || !file.name || file.name === "." || file.name === ".." || file.name.includes("/") || file.name.includes("\\") || [...file.name].some(character => character.charCodeAt(0) < 32) || file.size > 128 * 1024 * 1024) throw new Error("Choose a file up to 128 MB with a valid name")
    const filePath = `${path}/${file.name}`
    await request("create", filePath)
    try {
      for (let offset = 0; offset < file.size; offset += chunk) {
        const data = toBase64(new Uint8Array(await file.slice(offset, offset + chunk).arrayBuffer()))
        await request("write", filePath, { offset, data })
      }
      await request("truncate", filePath, { length: file.size })
    } catch (reason) {
      await request("remove", filePath).catch(() => undefined)
      throw reason
    }
    setRevision(value => value + 1)
  })

  return <div className="flex h-full min-h-0 flex-col gap-3 overflow-auto p-3 text-sm" data-connected-files>
    <div className="flex flex-wrap items-center gap-2">
      <h2 className="mr-auto text-sm font-semibold">Shared files</h2>
      {choice?.writable ? <label className="inline-flex cursor-pointer items-center gap-1 rounded-md border px-2 py-1 text-xs hover:bg-accent"><UploadIcon aria-hidden="true" className="size-3.5" />Upload file<input className="sr-only" type="file" disabled={busy} onChange={event => { const file = event.target.files?.[0]; event.target.value = ""; if (file) upload(file) }} /></label> : null}
      <Button aria-label="Refresh shared files" size="xs" variant="ghost" disabled={busy || loading} onClick={() => setRevision(value => value + 1)}><RefreshCwIcon aria-hidden="true" /></Button>
    </div>
    {!choices.length ? <p className="text-sm text-muted-foreground">No shared folders are connected to this environment. Select a folder in New connection, then save the link.</p> : <>
      <div className="flex flex-wrap gap-1" aria-label="Connected folders">{choices.map(item => <Button key={item.key} size="xs" variant={choice?.key === item.key ? "secondary" : "ghost"} onClick={() => choose(item.key)}>{item.label}</Button>)}</div>
      <div className="flex items-center gap-2 border-b pb-2 text-xs"><Button size="icon-xs" variant="ghost" aria-label="Parent shared folder" disabled={!parts.length || busy} onClick={() => { setParts(current => current.slice(0, -1)); setOpenFile(null) }}><ChevronLeftIcon aria-hidden="true" /></Button><span className="min-w-0 flex-1 truncate">{choice?.label}{parts.length ? ` / ${parts.join(" / ")}` : ""}</span><span className="text-muted-foreground">{choice?.writable ? "Read & write" : "Read only"}</span></div>
      {error ? <p role="alert" className="break-words text-xs text-destructive-foreground">{error}</p> : null}
      {loading ? <p role="status" className="text-xs text-muted-foreground">Loading shared files…</p> : <div className="flex min-h-0 flex-1 gap-3">
        <ul aria-label="Shared folder contents" className="w-60 shrink-0 overflow-auto rounded-md border p-1 text-xs">{[...entries].sort((a, b) => Number(b.directory) - Number(a.directory) || a.name.localeCompare(b.name)).map(entry => <li key={entry.name} className="flex items-center gap-1"><button className="flex min-w-0 flex-1 items-center gap-1 truncate rounded p-1 text-left hover:bg-accent" disabled={busy} onClick={() => entry.directory ? (setParts(current => [...current, entry.name]), setOpenFile(null)) : open(entry)}>{entry.directory ? <FolderIcon aria-hidden="true" className="size-3.5 shrink-0" /> : <FileIcon aria-hidden="true" className="size-3.5 shrink-0" />}<span className="truncate">{entry.name}</span></button>{!entry.directory ? <Button size="icon-xs" variant="ghost" aria-label={`Download ${entry.name}`} disabled={busy} onClick={() => download(entry)}><DownloadIcon aria-hidden="true" /></Button> : null}</li>)}{!entries.length ? <li className="p-2 text-muted-foreground">This folder is empty.</li> : null}</ul>
        <div className="flex min-w-0 flex-1 flex-col gap-2"><div className="flex items-center gap-2 text-xs"><span className="min-w-0 flex-1 truncate">{openFile?.name ?? "Select a file to view"}</span>{openFile && choice?.writable ? <Button size="xs" disabled={busy} onClick={save}><SaveIcon aria-hidden="true" />Save</Button> : null}</div><textarea aria-label="Shared file contents" className="min-h-40 flex-1 resize-none rounded-md border bg-background p-2 font-mono text-xs" value={openFile?.text ?? ""} readOnly={!choice?.writable || !openFile} onChange={event => setOpenFile(current => current ? { ...current, text: event.target.value } : current)} /></div>
      </div>}
    </>}
  </div>
}
