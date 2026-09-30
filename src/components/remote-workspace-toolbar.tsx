import { useEffect, useState } from "react"
import { AppWindowIcon, ChevronDownIcon, DownloadIcon, EllipsisIcon, Maximize2Icon, Minimize2Icon, PanelsTopLeftIcon, PlusIcon, SquareArrowOutUpRightIcon } from "lucide-react"
import type { Environment } from "@/types/platform"
import type { RemoteCapabilities } from "@/api/remote-access-api"
import { workspaceApi, type GuestWindow } from "@/api/workspace-api"
import { usePlatform } from "@/context/platform-context"
import { ConnectionSkillsDialog } from "@/components/connection-skills-dialog"
import { GuestAppLauncher } from "@/components/guest-app-launcher"
import { terminalInstallers, type TerminalInstallerId } from "@/lib/terminal-installers"
import { Button } from "@/components/ui/button"
import { Menu, MenuTrigger, MenuPopup, MenuItem, MenuSeparator, MenuGroup, MenuGroupLabel } from "@/components/ui/menu"

export function RemoteWorkspaceToolbar({ environment, capabilities, onInstall, onView, onClose, onError }: {
  environment: Environment; capabilities: RemoteCapabilities | null;
  onInstall(tool: TerminalInstallerId): void; onView(view: string): void;
  onClose(): void; onError(error: string): void;
}) {
  const { openEnvironmentWindow } = usePlatform()
  const [windows, setWindows] = useState<GuestWindow[]>([])
  const [loading, setLoading] = useState(false), [busy, setBusy] = useState(false)
  const [fullscreen, setFullscreen] = useState(Boolean(document.fullscreenElement))
  const running = environment.status === "running"
  useEffect(() => { const update = () => setFullscreen(Boolean(document.fullscreenElement)); document.addEventListener("fullscreenchange", update); return () => document.removeEventListener("fullscreenchange", update) }, [])
  const perform = async (action: () => Promise<unknown>) => { setBusy(true); try { await action() } catch (error) { onError(String(error)) } finally { setBusy(false) } }
  return <div className="ml-auto flex flex-wrap items-center gap-1" aria-label="Shared environment tools">
    {capabilities?.skills ? <ConnectionSkillsDialog environmentId={environment.id} remoteCanInstall={Boolean(capabilities.installers)} /> : <Button size="sm" variant="ghost" disabled title="Skills require Full Control permission">Skills</Button>}
    {capabilities?.apps ? <GuestAppLauncher environment={{ ...environment, kind: capabilities.appKind ?? "container" }} /> : <Button size="sm" variant="outline" disabled title="Graphical apps require Full Control and a supported container or MicroVM"><AppWindowIcon />Apps</Button>}
    <Menu modal={false}><MenuTrigger disabled={!running || !capabilities?.installers || !capabilities.internet} render={<Button aria-label="Install tools" title={!capabilities?.internet ? "The owner must enable Internet access before installing tools" : "Install tools in this shared environment"} size="sm" variant="ghost" className="text-xs text-muted-foreground" />}><DownloadIcon />Install tools<ChevronDownIcon className="size-3" /></MenuTrigger>
      <MenuPopup align="end"><MenuGroup><MenuGroupLabel>Install in this shared environment</MenuGroupLabel>{terminalInstallers.map(tool => <MenuItem key={tool.id} onClick={() => onInstall(tool.id)}>Install {tool.name}</MenuItem>)}</MenuGroup><MenuSeparator /><p className="max-w-64 px-2 py-1 text-xs text-muted-foreground">Runs in a dedicated terminal on the owner's environment.</p></MenuPopup>
    </Menu>
    <Button aria-label="New window" size="sm" variant="ghost" disabled={busy} className="bg-primary/10 text-xs text-primary" onClick={() => void perform(() => openEnvironmentWindow(environment.id))}><SquareArrowOutUpRightIcon />New window</Button>
    <Menu modal={false} onOpenChange={open => { if (!open) return; setLoading(true); void workspaceApi.windows().then(setWindows).catch(error => onError(String(error))).finally(() => setLoading(false)) }}>
      <MenuTrigger render={<Button aria-label="Switch window" size="sm" variant="ghost" disabled={busy} className="text-xs text-muted-foreground" />}><PanelsTopLeftIcon />Windows<ChevronDownIcon className="size-3" /></MenuTrigger>
      <MenuPopup align="end" className="w-72"><MenuGroup><MenuGroupLabel>Windows on this computer</MenuGroupLabel>{loading ? <p role="status" className="p-2 text-xs">Loading windows…</p> : windows.length ? windows.map(window => <MenuItem key={window.label} onClick={() => void perform(() => workspaceApi.focusWindow(window.label))}>{window.title}</MenuItem>) : <p className="p-2 text-xs text-muted-foreground">No other windows open.</p>}</MenuGroup><MenuSeparator /><MenuItem onClick={() => void perform(() => openEnvironmentWindow(environment.id))}><PlusIcon />New window</MenuItem></MenuPopup>
    </Menu>
    <Button aria-label="Toggle fullscreen" aria-pressed={fullscreen} size="icon-sm" variant="ghost" disabled={busy} onClick={() => void perform(() => document.fullscreenElement ? document.exitFullscreen() : document.documentElement.requestFullscreen())}>{fullscreen ? <Minimize2Icon /> : <Maximize2Icon />}</Button>
    <Menu modal={false}><MenuTrigger render={<Button aria-label="Workspace actions" size="icon-sm" variant="ghost" />}><EllipsisIcon /></MenuTrigger><MenuPopup align="end">
      <MenuItem disabled={!capabilities?.commands} onClick={() => onView("terminal")}>Terminal</MenuItem>
      <MenuItem disabled={!capabilities?.files} onClick={() => onView("files")}>Files and code editor</MenuItem>
      <MenuItem disabled={!capabilities || Boolean(capabilities.name)} onClick={() => onView("logs")}>Logs</MenuItem>
      <MenuItem disabled={!capabilities?.desktop} onClick={() => onView("desktop")}>Desktop</MenuItem>
      <MenuSeparator /><MenuItem onClick={onClose}>Close window</MenuItem>
    </MenuPopup></Menu>
  </div>
}
