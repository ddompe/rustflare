import { KeyboardEvent, useEffect, useRef, useState } from 'react'
import { api, Status, useStatus, reportError, useUiError, explainStatus, isCalibrationFailure } from './api'
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
import { ToneIcon, Tone } from './components/ui'
import { SetupStrip, SetupStepId } from './components/SetupStrip'
import { holdLevel } from './vocab'
import { useSaved } from './saved'

const TABS: { id: TabId; label: string }[] = [
  { id: 'camera', label: 'Camera' },
  { id: 'aim', label: 'Aim' },
  { id: 'track', label: 'Track' },
  { id: 'record', label: 'Record' },
  { id: 'more', label: 'More' },
]



export function App() {
  const { status, connected } = useStatus()
  const [anchorPick, setAnchorPick] = useState(false)
  const [filesOpen, setFilesOpen] = useState(false)
  // Files opens in the folder it was last in, or in `path` when a screen sends you to one (the stacks)
  const openFiles = (path?: string) => { if (path !== undefined) sessionStorage.setItem('rf-fm-cwd', path); setFilesOpen(true) }
  const [display, setDisplay, resetDisplay] = useDisplay()
  const [savedTab, setSavedTab] = useSetting<TabId>('ui.tab', 'camera')
  const [tab, setTabState] = useState<TabId>(TABS.some((x) => x.id === savedTab) ? savedTab : 'camera')
  const setTab = (t: TabId) => { setTabState(t); setSavedTab(t) }
  const [focus, setFocus] = useState<{ id: string; nonce: number } | null>(null)
  /** Which section a tab should open when it is entered from elsewhere (the Aim tab's Mount time and place). */
  const [aimFocus, setAimFocus] = useState<{ id: string; nonce: number } | null>(null)
  const [full, setFull] = useState(false)
  const [theme, setTheme] = useSetting<'night' | 'day'>('ui.theme', 'night')
  useEffect(() => { document.documentElement.dataset.theme = theme }, [theme])
  const tabRefs = useRef<Record<string, HTMLButtonElement | null>>({})
  const panelRef = useRef<HTMLElement>(null)
  useEffect(() => { panelRef.current?.scrollTo(0, 0) }, [tab])
  const cam = status?.camera
  const [saved, dismissSaved] = useSaved(status)

  const goto = (t: TabId, f?: string) => { if (f && t === 'aim') setAimFocus({ id: f, nonce: Date.now() }); setTab(t) }
  const openMore = (id: string) => { setFocus({ id, nonce: Date.now() }); setTab('more') }
  const onSetupStep = (id: SetupStepId) => { if (id === 'optics') openMore('optics'); else if (id === 'site') openMore('site'); else if (id === 'mount') goto('aim', 'clock'); else goto('camera') }
  const stopCapture = () => { api('POST', '/capture/stop').catch((e) => reportError(e, 'Stop')) }

  // A mark on each tab says where the session stands without opening anything, and why (it is the tab's title and screen-reader text too).
  // A tab is ok only when its job is done; it warns only for something the user can act on.
  const g = status?.guiding
  const t = status?.tracking
  const level = holdLevel(g?.error_px)
  // "still settling" is only a problem when it goes on: the guider needs a minute to pull in after a slew
  const settleSince = useRef<number | null>(null)
  if (g?.state === 'guiding' && level === 'settling') { if (settleSince.current == null) settleSince.current = Date.now() } else settleSince.current = null
  const settlingLong = settleSince.current != null && Date.now() - settleSince.current > 60_000
  const tone = (id: TabId): { tone: Tone; reason: string } | null => {
    if (id === 'camera') {
      if (!cam) return null
      return cam.saturated > 0.001 ? { tone: 'bad', reason: 'the picture is clipping: lower the exposure' } : { tone: 'ok', reason: 'camera open, no clipping' }
    }
    if (id === 'aim') {
      const m = status?.mount
      if (!m) return null
      const st = m.site_time
      if (m.link_up === false) return { tone: 'warn', reason: 'the link to the mount is down' }
      if (m.park === 'parked' || m.park === 'parking') return { tone: 'warn', reason: 'the mount is parked' }
      if (st && st.ok === false) return { tone: 'warn', reason: 'the mount clock or place differs from the Pi' }
      if (!m.tracking) return { tone: 'warn', reason: 'mount tracking is off' }
      return { tone: 'ok', reason: 'mount connected and tracking, time matches' }
    }
    if (id === 'track') {
      if (t?.lost) return { tone: 'bad', reason: 'the target is lost' }
      if (isCalibrationFailure(status?.error) && !g?.calibration) return { tone: 'warn', reason: 'calibration failed' }
      if (g?.state === 'guiding') {
        if (level === 'off') return { tone: 'warn', reason: `off target by ${g.error_px?.toFixed(0)} px` }
        if (level === 'settling' && settlingLong) return { tone: 'warn', reason: 'still settling after a minute' }
        if (level === 'locked') return { tone: 'ok', reason: 'holding the target' }
      }
      return null
    }
    if (id === 'record') return status && status.capture.state !== 'idle' ? { tone: 'rec', reason: 'recording' } : null
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
    <div className="app" data-tab={tab} data-full={full ? '1' : undefined} data-nocam={cam ? undefined : '1'}>
      <a className="skip" href="#panel">Skip to controls</a>
      <header className="top">
        <img className="mark" src="/logo-mark.svg" alt="" width="26" height="26" />
        <h1>Rust<span>Flare</span></h1>
        <span className={`conn ${connected ? 'ok' : 'bad'}`}><i aria-hidden="true" />{connected ? 'Online' : 'Reconnecting…'}</span>
        {cam && <span className="camname">{cam.name} · {cam.width}×{cam.height} · {cam.bits}-bit{cam.bayer ? ` · colour ${cam.bayer}` : ''}</span>}
        <span className="hright">
          <button type="button" className="hfiles" onClick={() => setFilesOpen(true)} aria-label="Files: recordings and stacks">Files</button>
          <button type="button" className="themebtn" onClick={() => setTheme(theme === 'night' ? 'day' : 'night')} aria-label={theme === 'night' ? 'Switch to the daylight theme' : 'Switch to the night theme'}>
            {theme === 'night'
              ? <svg viewBox="0 0 20 20" width="20" height="20" aria-hidden="true"><circle cx="10" cy="10" r="3.6" fill="currentColor" /><path d="M10 1.8v2.4M10 15.8v2.4M1.8 10h2.4M15.8 10h2.4M4.2 4.2l1.7 1.7M14.1 14.1l1.7 1.7M4.2 15.8l1.7-1.7M14.1 5.9l1.7-1.7" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" fill="none" /></svg>
              : <svg viewBox="0 0 20 20" width="20" height="20" aria-hidden="true"><path d="M16.5 12.2A7 7 0 0 1 7.8 3.5a7 7 0 1 0 8.7 8.7z" fill="currentColor" /></svg>}
          </button>
          <button type="button" className="hidebtn" onClick={() => setFull(true)}>Hide controls</button>
        </span>
      </header>
      <section className="view" aria-label="Live view">
        <div className="live-slot">
          <LiveView status={status} display={display} setDisplay={setDisplay} anchorPick={anchorPick} onPicked={() => setAnchorPick(false)} onDisplay={() => openMore('display')} />
        </div>
        <StatusStrip status={status} goto={goto} onStop={stopCapture} hideRec={tab === 'record'} hide={tab === 'aim' ? ['sun'] : []} saved={saved} onSavedOpen={() => { openFiles(saved?.dir ?? ''); }} onSavedDismiss={dismissSaved} setup={tab !== 'camera' ? <SetupStrip status={status} onStep={onSetupStep} variant="pill" /> : undefined} />
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
              title={tn ? `${x.label}: ${toneText[tn.tone]}, ${tn.reason}` : undefined}
              onClick={() => setTab(x.id)}
            >
              <span>{x.label}</span>
              {tn && <ToneIcon tone={tn.tone} size={13} />}
              {tn && <span className="sr-only"> ({toneText[tn.tone]}: {tn.reason})</span>}
            </button>
          )
        })}
      </nav>

      <main ref={panelRef} className="panel" id="panel" role="tabpanel" aria-label={TABS.find((x) => x.id === tab)?.label} tabIndex={-1}>
        {tab === 'camera' && <CameraTab status={status} next={() => goto('aim')} setup={<SetupStrip status={status} onStep={onSetupStep} variant="bar" />} />}
        {tab === 'aim' && <AimTab status={status} next={() => goto('track')} openMore={openMore} focus={aimFocus} />}
        {tab === 'track' && <TrackTab status={status} anchorPick={anchorPick} setAnchorPick={setAnchorPick} goto={() => goto('aim')} next={() => goto('record')} toCamera={() => setTab('camera')} />}
        {tab === 'record' && <RecordTab status={status} goto={(_t, f) => openMore(f)} openFiles={openFiles} saved={saved} dismissSaved={dismissSaved} toCamera={() => setTab('camera')} />}
        {tab === 'more' && <MoreTab status={status} display={display} setDisplay={setDisplay} resetDisplay={resetDisplay} focus={focus} openFiles={openFiles} />}
      </main>
      <StatusBar status={status} connected={connected} />
      {full && status && status.capture.state !== 'idle' && (
        <button type="button" className="fullrec" onClick={() => api('POST', '/capture/stop').catch((e) => reportError(e, 'Stop'))}><ToneIcon tone="rec" size={12} /> {status.capture.state === 'flushing' ? 'Saving' : 'Recording'}{status.capture.total ? ` ${status.capture.done}/${status.capture.total}` : ` ${status.capture.done}`} · Stop</button>
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
  else if (status?.error && !(lost && /^tracking lost/i.test(status.error))) { tone = 'bad'; text = explainStatus(status.error) }
  else if (status?.auto_flip?.message && (status.auto_flip.stage === 'flipping' || status.auto_flip.stage === 'done')) { tone = 'warn'; text = status.auto_flip.message } // an active flip explains everything else
  else if (status?.capture?.stopped_early) { tone = 'bad'; text = status.capture.stopped_early }
  else if (lost && status?.guiding?.dead_reckoning) { tone = 'warn'; text = 'Sun not visible: guiding goes on from the measured drift to keep the framing.' }
  else if (lost) { tone = 'bad'; text = held ? 'Target lost: guiding is paused and resumes by itself when the limb is back in view.' : 'Target lost: slew it back into view (Aim) or restart tracking.' }
  else if (held) { tone = 'warn'; text = 'Guiding is paused until the target is found again.' }
  else if (status?.sun_held) { tone = 'warn'; text = 'Sun hidden or unclear (cloud?): holding its last position.' }
  else if (status?.auto_flip?.message && status.auto_flip.stage !== 'off') { tone = 'warn'; text = status.auto_flip.message }
  return (
    <div className={`sbar ${tone}`} role="status" aria-live="polite">
      {text ? (
        <button type="button" className="sbar-btn" aria-expanded={open} aria-label={`${text}. Tap to read all of it.`} onClick={() => setOpen(!open)}>
          <i aria-hidden="true" /><span aria-hidden="true">{text}</span>
        </button>
      ) : <span className="sbar-btn"><span className="sbar-idle">Ready</span></span>}
      {status?.version && <span className="sbar-ver" title="RustFlare version">v{status.version}</span>}
      {uiError && <button type="button" className="sbar-dismiss" onClick={() => { setOpen(false); clearUiError() }}>Dismiss</button>}
      {!uiError && notices.length > 0 && <button type="button" className="sbar-dismiss" onClick={() => { setOpen(false); void dismiss() }}>{notices.length > 1 ? 'Dismiss all' : 'Dismiss'}</button>}
      {open && text && (notices.length
        ? <div className="sbar-pop">{notices.map((n) => <p key={n.id} className={n.level}><b>{n.at}</b> {n.text} <button type="button" className="link" onClick={() => void dismiss(n.id)}>Dismiss</button></p>)}</div>
        : <div className="sbar-pop" onClick={() => setOpen(false)}>{text}</div>)}
    </div>
  )
}
