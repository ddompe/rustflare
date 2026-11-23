import { KeyboardEvent, useEffect, useRef, useState } from 'react'
import { api, Status, useStatus, reportError, useUiError } from './api'
import { useDisplay } from './display'
import { useSetting } from './settings'
import { LiveView } from './components/LiveView'
import { StatusStrip, TabId } from './components/StatusStrip'
import { CameraTab } from './components/CameraTab'
import { AimTab } from './components/AimTab'
import { TrackTab } from './components/TrackTab'
import { RecordTab } from './components/RecordTab'
import { MoreTab } from './components/MoreTab'
import { FileManagerScreen } from './components/FileManager'

const TABS: { id: TabId; label: string }[] = [
  { id: 'camera', label: 'Camera' },
  { id: 'aim', label: 'Aim' },
  { id: 'track', label: 'Track' },
  { id: 'record', label: 'Record' },
  { id: 'more', label: 'More' },
]

type Tone = 'ok' | 'warn' | 'bad' | 'rec' | null

export function App() {
  const { status, connected } = useStatus()
  const [anchorPick, setAnchorPick] = useState(false)
  const [filesOpen, setFilesOpen] = useState(false)
  const [display, setDisplay, resetDisplay] = useDisplay()
  const [savedTab, setSavedTab] = useSetting<TabId>('ui.tab', 'camera')
  const [tab, setTabState] = useState<TabId>(TABS.some((x) => x.id === savedTab) ? savedTab : 'camera')
  const setTab = (t: TabId) => { setTabState(t); setSavedTab(t) }
  const [focus, setFocus] = useState<{ id: string; nonce: number } | null>(null)
  const [full, setFull] = useState(false)
  const [theme, setTheme] = useSetting<'night' | 'day'>('ui.theme', 'night')
  useEffect(() => { document.documentElement.dataset.theme = theme }, [theme])
  const tabRefs = useRef<Record<string, HTMLButtonElement | null>>({})
  const panelRef = useRef<HTMLElement>(null)
  useEffect(() => { panelRef.current?.scrollTo(0, 0) }, [tab])
  const cam = status?.camera

  const goto = (t: TabId) => setTab(t)
  const openMore = (id: string) => { setFocus({ id, nonce: Date.now() }); setTab('more') }
  const stopCapture = () => { api('POST', '/capture/stop').catch((e) => reportError(e, 'Stop')) }

  // A dot on each tab says where the session stands without opening anything.
  const g = status?.guiding
  const t = status?.tracking
  const tone = (id: TabId): Tone => {
    if (id === 'camera') return !cam ? null : cam.saturated > 0.001 ? 'bad' : 'ok'
    if (id === 'aim') return !status?.mount ? null : status.mount.tracking ? 'ok' : 'warn'
    if (id === 'track') return t?.lost ? 'bad' : g?.state === 'guiding' ? 'ok' : t ? 'warn' : null
    if (id === 'record') return status && status.capture.state !== 'idle' ? 'rec' : null
    return null
  }
  const toneText: Record<string, string> = { ok: 'ready', warn: 'needs attention', bad: 'problem', rec: 'recording' }

  const onKey = (e: KeyboardEvent) => {
    const i = TABS.findIndex((x) => x.id === tab)
    const to = e.key === 'ArrowRight' ? (i + 1) % TABS.length : e.key === 'ArrowLeft' ? (i + TABS.length - 1) % TABS.length : e.key === 'Home' ? 0 : e.key === 'End' ? TABS.length - 1 : -1
    if (to < 0) return
    e.preventDefault()
    setTab(TABS[to].id)
    tabRefs.current[TABS[to].id]?.focus()
  }

  return (
    <div className="app" data-tab={tab} data-full={full ? '1' : undefined}>
      <a className="skip" href="#panel">Skip to controls</a>
      <header className="top">
        <img className="mark" src="/logo-mark.svg" alt="" width="26" height="26" />
        <h1>Rust<span>Flare</span></h1>
        <span className={`conn ${connected ? 'ok' : 'bad'}`}><i aria-hidden="true" />{connected ? 'Online' : 'Reconnecting…'}</span>
        {cam && <span className="camname">{cam.name} · {cam.width}×{cam.height} · {cam.bits}-bit{cam.bayer ? ` · colour ${cam.bayer}` : ''}</span>}
        <span className="hright">
          <button type="button" className="hfiles" onClick={() => setFilesOpen(true)} aria-label="Files: recordings on the drive">Files</button>
          <button type="button" className="themebtn" onClick={() => setTheme(theme === 'night' ? 'day' : 'night')} aria-label={theme === 'night' ? 'Switch to the daylight theme' : 'Switch to the night theme'}>{theme === 'night' ? '☀' : '☾'}</button>
          <button type="button" className="hidebtn" onClick={() => setFull(true)}>Hide controls</button>
        </span>
      </header>
      <section className="view" aria-label="Live view">
        <LiveView status={status} display={display} setDisplay={setDisplay} anchorPick={anchorPick} onPicked={() => setAnchorPick(false)} onDisplay={() => openMore('display')} />
        <StatusStrip status={status} goto={goto} onStop={stopCapture} hideRec={tab === 'record'} noQuick={tab === 'aim'} hide={tab === 'aim' ? ['sun'] : []} />
      </section>

      <nav className="tabs" role="tablist" aria-label="Session steps" onKeyDown={onKey}>
        {TABS.map((x) => {
          const tn = tone(x.id)
          return (
            <button
              key={x.id}
              ref={(el) => { tabRefs.current[x.id] = el }}
              type="button"
              role="tab"
              id={`tab-${x.id}`}
              aria-selected={tab === x.id}
              aria-controls="panel"
              tabIndex={tab === x.id ? 0 : -1}
              className={tab === x.id ? 'on' : ''}
              onClick={() => setTab(x.id)}
            >
              <span>{x.label}</span>
              {tn && <i className={`tdot ${tn}`} aria-hidden="true" />}
              {tn && <span className="sr-only"> ({toneText[tn]})</span>}
            </button>
          )
        })}
      </nav>

      <main ref={panelRef} className="panel" id="panel" role="tabpanel" aria-labelledby={`tab-${tab}`} tabIndex={-1}>
        {tab === 'camera' && <CameraTab status={status} next={() => goto('aim')} />}
        {tab === 'aim' && <AimTab status={status} next={() => goto('track')} openMore={openMore} />}
        {tab === 'track' && <TrackTab status={status} anchorPick={anchorPick} setAnchorPick={setAnchorPick} goto={() => goto('aim')} next={() => goto('record')} />}
        {tab === 'record' && <RecordTab status={status} goto={(_t, f) => openMore(f)} openFiles={() => setFilesOpen(true)} />}
        {tab === 'more' && <MoreTab status={status} display={display} setDisplay={setDisplay} resetDisplay={resetDisplay} focus={focus} />}
      </main>
      <StatusBar status={status} connected={connected} />
      {full && status && status.capture.state !== 'idle' && (
        <button type="button" className="fullrec" onClick={() => api('POST', '/capture/stop').catch((e) => reportError(e, 'Stop'))}>● {status.capture.state === 'flushing' ? 'Saving' : 'Recording'}{status.capture.total ? ` ${status.capture.done}/${status.capture.total}` : ` ${status.capture.done}`} · Stop</button>
      )}
      {full && <button type="button" className="fullbtn" aria-pressed onClick={() => setFull(false)}>Show controls</button>}
      {filesOpen && <FileManagerScreen status={status} onClose={() => setFilesOpen(false)} />}
    </div>
  )
}

/**
 * One reserved line at the bottom for messages (errors, lost target, connection), so showing or hiding one never moves the
 * controls. Tap a long message to read all of it (it opens upwards over the page, it does not push anything).
 */
function StatusBar({ status, connected }: { status: Status | null; connected: boolean }) {
  const [open, setOpen] = useState(false)
  const lost = !!status?.tracking?.lost
  const held = !!status?.guiding?.held
  let tone: 'bad' | 'warn' | '' = ''
  let text = ''
  // things the user has to know about stay until dismissed, newest first
  const notices = (status?.notices ?? []).filter((n) => !n.acked).reverse()
  const [uiError, clearUiError] = useUiError()
  const dismiss = (id?: number) => api('POST', '/notices/ack', id === undefined ? {} : { id }).catch((e) => reportError(e, 'Dismissing the message'))
  if (!connected && !status) { tone = 'warn'; text = 'Connecting to the RustFlare server…' }
  else if (uiError) { tone = 'bad'; text = uiError }
  else if (notices.length) { tone = notices.some((n) => n.level === 'error') ? 'bad' : 'warn'; text = notices[0].text + (notices.length > 1 ? ` (+${notices.length - 1} more)` : '') }
  else if (status?.error && !(lost && /^tracking lost/i.test(status.error))) { tone = 'bad'; text = status.error }
  else if (status?.auto_flip?.message && (status.auto_flip.stage === 'flipping' || status.auto_flip.stage === 'done')) { tone = 'warn'; text = status.auto_flip.message } // an active flip explains everything else
  else if (status?.capture?.stopped_early) { tone = 'bad'; text = status.capture.stopped_early }
  else if (lost && status?.guiding?.dead_reckoning) { tone = 'warn'; text = 'Sun not visible: guiding goes on from the measured drift to keep the framing.' }
  else if (lost) { tone = 'bad'; text = held ? 'Target lost: guiding is paused and resumes by itself when the limb is back in view.' : 'Target lost: slew it back into view (Aim) or restart tracking.' }
  else if (held) { tone = 'warn'; text = 'Guiding is paused until the target is found again.' }
  else if (status?.sun_held) { tone = 'warn'; text = 'Sun hidden or unclear (cloud?): holding its last position.' }
  else if (status?.auto_flip?.message && status.auto_flip.stage !== 'off') { tone = 'warn'; text = status.auto_flip.message }
  return (
    <div className={`sbar ${tone}`} role="status" aria-live="polite">
      <button type="button" className="sbar-btn" disabled={!text} aria-expanded={open} onClick={() => setOpen(!open)}>
        {text ? <><i aria-hidden="true" /><span>{text}</span></> : <span className="sbar-idle">Ready</span>}
      </button>
      {uiError && <button type="button" className="sbar-dismiss" onClick={() => { setOpen(false); clearUiError() }}>Dismiss</button>}
      {!uiError && notices.length > 0 && <button type="button" className="sbar-dismiss" onClick={() => { setOpen(false); void dismiss() }}>{notices.length > 1 ? 'Dismiss all' : 'Dismiss'}</button>}
      {open && text && (notices.length
        ? <div className="sbar-pop">{notices.map((n) => <p key={n.id} className={n.level}><b>{n.at}</b> {n.text} <button type="button" className="link" onClick={() => void dismiss(n.id)}>Dismiss</button></p>)}</div>
        : <div className="sbar-pop" onClick={() => setOpen(false)}>{text}</div>)}
    </div>
  )
}
