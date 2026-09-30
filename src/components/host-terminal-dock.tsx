import { useCallback, useEffect, useRef, useState } from "react"
import { AlertCircleIcon, ArrowUpRightIcon, CheckIcon, ChevronDownIcon, CopyIcon, FolderOpenIcon, LoaderCircleIcon, Maximize2Icon, Minimize2Icon, PlusIcon, TerminalIcon, Trash2Icon, XIcon } from "lucide-react"
import { hostTerminalApi, type HostTerminalInfo } from "@/api/host-terminal-api"
import { terminalClipboard } from "@/lib/terminal-clipboard"
import { HostTerminalCanvas, type HostCanvasControls, type HostShellState, type HostTab } from "@/components/host-terminal-canvas"
import "./host-terminal-dock.css"

const message = (reason: unknown) => reason instanceof Error ? reason.message : String(reason)
const maximumHeight = () => Math.max(200, window.innerHeight - 96)
const clampHeight = (height: number) => Math.min(maximumHeight(), Math.max(250, height))

export default function HostTerminalDock({ visible, height, onHeightChange, onHide }: {
  visible: boolean; height: number; onHeightChange(height: number): void; onHide(): void
}) {
  const [info, setInfo] = useState<HostTerminalInfo | null>(null)
  const [tabs, setTabs] = useState<HostTab[]>([])
  const [activeId, setActiveId] = useState("")
  const [workingDirectory, setWorkingDirectory] = useState<string>()
  const [loading, setLoading] = useState(true)
  const [retry, setRetry] = useState(0)
  const [error, setError] = useState("")
  const [notice, setNotice] = useState("")
  const [settingUp, setSettingUp] = useState(false)
  const [choosingFolder, setChoosingFolder] = useState(false)
  const [ending, setEnding] = useState("")
  const [closing, setClosing] = useState(false)
  const [maximized, setMaximized] = useState(false)
  const previousHeight = useRef(height)
  const counter = useRef(0)
  const controls = useRef(new Map<string, HostCanvasControls>())
  const drag = useRef<{ y: number; height: number } | null>(null)
  const active = tabs.find(tab => tab.id === activeId)
  const limit = info?.maxSessions ?? 4
  const canCreate = Boolean(info && !info.elevated && tabs.length < limit && !closing)

  const addTab = useCallback((cwd: string, shell: string, command?: HostTab["command"]) => {
    const id = `host-${crypto.randomUUID()}`
    const name = command && command !== "yougori-cli help" ? `${command.charAt(0).toUpperCase()}${command.slice(1)}` : `${shell} ${++counter.current}`
    setTabs(current => [...current, { id, name, cwd, command, state: "starting" }])
    setActiveId(id); setEnding(""); setNotice("")
  }, [])

  useEffect(() => {
    let disposed = false
    setLoading(true); setError("")
    void hostTerminalApi.info().then(result => {
      if (disposed) return
      setInfo(result)
      if (result.sessions.length) {
        const restored: HostTab[] = result.sessions.map(session => ({ id: session.sessionId, name: `${result.shell} ${++counter.current}`, cwd: session.cwd, reconnect: true, state: "starting" }))
        setTabs(restored); setActiveId(restored[0]?.id ?? "")
      } else if (!result.elevated) addTab(result.cwd, result.shell)
    }).catch(reason => { if (!disposed) setError(message(reason)) }).finally(() => { if (!disposed) setLoading(false) })
    return () => { disposed = true }
  }, [retry, addTab])

  useEffect(() => {
    const resize = () => onHeightChange(maximized ? maximumHeight() : clampHeight(height))
    resize(); window.addEventListener("resize", resize)
    return () => window.removeEventListener("resize", resize)
  }, [height, maximized, onHeightChange])

  const setShellState = useCallback((id: string, state: HostShellState) => {
    setTabs(current => current.map(tab => tab.id === id && tab.state !== state ? { ...tab, state } : tab))
  }, [])
  const setControls = useCallback((id: string, value: HostCanvasControls | null) => {
    if (value) controls.current.set(id, value); else controls.current.delete(id)
  }, [])
  // Restoring an old home-folder tab must not make new shells/agents start there.
  // Only an explicit folder-picker choice overrides the dedicated workspace.
  const start = (command?: HostTab["command"], cwd = workingDirectory ?? info?.cwd) => {
    if (info && canCreate && cwd) addTab(cwd, info.shell, command)
  }
  const setup = async () => {
    if (settingUp) return
    setSettingUp(true); setError(""); setNotice("")
    try {
      const skill = await hostTerminalApi.setup()
      setInfo(current => current ? { ...current, skill } : current)
      setNotice(skill.message)
    } catch (reason) { setError(message(reason)) }
    finally { setSettingUp(false) }
  }
  const chooseFolder = async () => {
    setChoosingFolder(true); setError("")
    try { const cwd = await hostTerminalApi.chooseFolder(); if (cwd) { setWorkingDirectory(cwd); start(undefined, cwd) } }
    catch (reason) { setError(message(reason)) }
    finally { setChoosingFolder(false) }
  }
  const copyGuide = async () => {
    if (!info) return
    try { await terminalClipboard.writeText(info.agentInstructions); setNotice("Agent guide copied. Paste it into your AI agent.") }
    catch (reason) { setError(message(reason)) }
  }
  const endTab = async () => {
    if (!ending || closing) return
    const id = ending
    setClosing(true); setError("")
    try {
      await hostTerminalApi.terminal({ sessionId: id, action: "close" })
      const remaining = tabs.filter(tab => tab.id !== id)
      setTabs(remaining)
      if (activeId === id) setActiveId(remaining[Math.max(0, tabs.findIndex(tab => tab.id === id) - 1)]?.id ?? remaining[0]?.id ?? "")
      setEnding("")
    } catch (reason) { setError(message(reason)) }
    finally { setClosing(false) }
  }
  const maximize = () => {
    if (maximized) { onHeightChange(clampHeight(previousHeight.current)); setMaximized(false) }
    else { previousHeight.current = height; onHeightChange(maximumHeight()); setMaximized(true) }
  }
  const changeTab = (index: number) => {
    const next = tabs[(index + tabs.length) % tabs.length]
    if (!next) return
    setActiveId(next.id); document.getElementById(`tab-${next.id}`)?.focus()
  }

  return <section id="host-terminal-panel" aria-label="Host terminal" className="host-terminal-dock" style={{ height, display: visible ? undefined : "none" }}>
    <div role="separator" aria-label="Resize host terminal" aria-orientation="horizontal" aria-valuemin={Math.min(250, maximumHeight())} aria-valuemax={maximumHeight()} aria-valuenow={height} tabIndex={0} className="host-terminal-resizer"
      onPointerDown={event => { if (event.button !== 0) return; drag.current = { y: event.clientY, height }; event.currentTarget.setPointerCapture(event.pointerId); setMaximized(false); event.preventDefault() }}
      onPointerMove={event => { if (drag.current) onHeightChange(clampHeight(drag.current.height + drag.current.y - event.clientY)) }}
      onPointerUp={event => { drag.current = null; if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId) }}
      onPointerCancel={() => { drag.current = null }}
      onKeyDown={event => { if (event.key === "ArrowUp" || event.key === "ArrowDown") { event.preventDefault(); setMaximized(false); onHeightChange(clampHeight(height + (event.key === "ArrowUp" ? 24 : -24))) } }}><span /></div>
    <div className="host-terminal-toolbar">
      <div className="host-terminal-identity"><TerminalIcon aria-hidden="true" /><strong>Host terminal</strong><span className="host-terminal-boundary" title="The workspace is only a starting folder. Host commands can still access other files allowed by your Windows or OS account.">Your computer · Not isolated</span></div>
      <div className="host-terminal-actions">
        <button type="button" title="New terminal in a project folder" aria-label="New terminal in a project folder" disabled={!canCreate || choosingFolder} onClick={() => void chooseFolder()}>{choosingFolder ? <LoaderCircleIcon className="host-terminal-spin" /> : <FolderOpenIcon />}</button>
        <button type="button" title="Clear visible terminal history" aria-label="Clear terminal history" disabled={!active} onClick={() => controls.current.get(activeId)?.clear()}><Trash2Icon /></button>
        <span className="host-terminal-divider" />
        <button type="button" title={maximized ? "Restore terminal size" : "Maximize terminal"} aria-label={maximized ? "Restore terminal size" : "Maximize terminal"} onClick={maximize}>{maximized ? <Minimize2Icon /> : <Maximize2Icon />}</button>
        <button type="button" title="Hide terminal — commands keep running" aria-label="Hide terminal" onClick={onHide}><ChevronDownIcon /></button>
      </div>
    </div>
    {info ? <div className="host-terminal-setup">
      <div className="host-terminal-setup-main">
        {info.skill.state === "ready" ? <span className="host-terminal-ready"><CheckIcon aria-hidden="true" />AI access ready</span> : <button type="button" className="host-terminal-primary" disabled={settingUp || info.skill.state === "conflict"} onClick={() => void setup()}>{settingUp ? <LoaderCircleIcon className="host-terminal-spin" /> : <PlusIcon />}<span>{settingUp ? "Setting up…" : info.skill.state === "updateAvailable" ? "Update AI agent access" : "Set up AI agent access"}</span></button>}
        <span className="host-terminal-setup-hint" title={info.skill.path}>{info.skill.state === "ready" ? "Start a new Codex session to load the skill." : info.skill.state === "conflict" ? "Your existing skill was left unchanged." : "Install Yougori’s Codex skill for your user account."}</span>
      </div>
      <div className="host-terminal-agent-actions">
        <button type="button" title="Copy Yougori instructions for any AI agent" onClick={() => void copyGuide()}><CopyIcon aria-hidden="true" /><span>Copy agent guide</span></button>
        {info.agents.filter(agent => agent.available).map(agent => <button key={agent.id} type="button" disabled={!canCreate} title={`Start ${agent.name} in ${workingDirectory ?? info.cwd}. Uses your installed agent and its login; this is not an isolated environment.`} aria-label={`Start ${agent.name}`} onClick={() => start(agent.id)}><span>{agent.name}</span><ArrowUpRightIcon aria-hidden="true" /></button>)}
      </div>
    </div> : null}
    {info?.elevated ? <div role="alert" className="host-terminal-notice is-error"><AlertCircleIcon />Reopen Yougori normally, not as administrator, to use the host terminal.</div> : null}
    {info?.skill.state === "conflict" ? <div role="alert" className="host-terminal-notice is-error"><AlertCircleIcon /><span>{info.skill.message} <span className="host-terminal-path">{info.skill.path}</span></span></div> : null}
    {error ? <div role="alert" className="host-terminal-notice is-error"><AlertCircleIcon /><span>{error}</span>{!info ? <button type="button" onClick={() => setRetry(value => value + 1)}>Retry</button> : <button type="button" aria-label="Dismiss terminal error" onClick={() => setError("")}><XIcon /></button>}</div> : null}
    {notice ? <div role="status" className="host-terminal-notice"><CheckIcon /><span>{notice}</span><button type="button" aria-label="Dismiss terminal notice" onClick={() => setNotice("")}><XIcon /></button></div> : null}
    <div className="host-terminal-tabs-bar">
      <div className="host-terminal-tabs" role="tablist" aria-label="Host terminal tabs">
        {tabs.map((tab, index) => <div key={tab.id} className={`host-terminal-tab${activeId === tab.id ? " is-active" : ""}`}>
          <button type="button" role="tab" id={`tab-${tab.id}`} aria-controls={`pane-${tab.id}`} aria-selected={activeId === tab.id} tabIndex={activeId === tab.id ? 0 : -1} onClick={() => setActiveId(tab.id)} onKeyDown={event => { if (event.key === "ArrowRight" || event.key === "ArrowLeft") { event.preventDefault(); changeTab(index + (event.key === "ArrowRight" ? 1 : -1)) } }}>
            {tab.state === "starting" ? <LoaderCircleIcon aria-hidden="true" className="host-terminal-spin" /> : <span aria-hidden="true" className={`host-terminal-dot is-${tab.state}`} />}<span>{tab.name}</span>
          </button>
          <button type="button" title={`End ${tab.name}`} aria-label={`End ${tab.name}`} disabled={closing} onClick={() => setEnding(tab.id)}><XIcon aria-hidden="true" /></button>
        </div>)}
      </div>
      <button type="button" className="host-terminal-new" aria-label="New host terminal" title={canCreate ? "New host terminal" : `Up to ${limit} host terminals`} disabled={!canCreate} onClick={() => start()}><PlusIcon /></button>
      <span className="host-terminal-tab-count">{tabs.length} / {limit}</span>
    </div>
    {ending ? <div role="alertdialog" aria-label="End host terminal" className="host-terminal-confirm"><span>End <strong>{tabs.find(tab => tab.id === ending)?.name}</strong>? This stops its shell{info?.shell === "PowerShell" ? " and child processes" : ""}.</span><button type="button" disabled={closing} onClick={() => setEnding("")}>Cancel</button><button type="button" className="host-terminal-danger" disabled={closing} onClick={() => void endTab()}>{closing ? <LoaderCircleIcon className="host-terminal-spin" /> : <XIcon />}End terminal</button></div> : null}
    <div className="host-terminal-content">
      {loading ? <div className="host-terminal-empty" role="status"><LoaderCircleIcon className="host-terminal-spin" /><span>Opening your command line…</span></div> : null}
      {!loading && !tabs.length && info && !info.elevated ? <div className="host-terminal-empty"><TerminalIcon /><span>Your command line, inside Yougori.</span><small>Runs on your computer. Guest terminals stay in their environments.</small><button type="button" className="host-terminal-primary" onClick={() => start()}>New terminal</button></div> : null}
      {tabs.map(tab => <div key={tab.id} id={`pane-${tab.id}`} role="tabpanel" aria-labelledby={`tab-${tab.id}`} hidden={activeId !== tab.id} className="host-terminal-pane"><HostTerminalCanvas tab={tab} active={visible && activeId === tab.id} onState={setShellState} onControls={setControls} />{tab.state === "starting" ? <div className="host-terminal-starting" role="status"><LoaderCircleIcon className="host-terminal-spin" />Starting {tab.name}…</div> : null}</div>)}
    </div>
    <div className="host-terminal-footer"><span className="host-terminal-working-directory" title={active?.cwd ?? info?.cwd}><FolderOpenIcon aria-hidden="true" />{active?.cwd ?? info?.cwd ?? "Your computer"}</span><span>{active?.state === "exited" ? "Shell exited" : "Ctrl+C interrupt · Ctrl+V paste"}</span><span title={info?.cliPath}>{info ? "Yougori CLI ready" : loading ? "Loading CLI…" : "CLI unavailable"}</span></div>
  </section>
}
