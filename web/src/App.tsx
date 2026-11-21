import { KeyboardEvent, useEffect, useRef, useState } from 'react'
import { api, useStatus } from './api'
import { useDisplay } from './display'
import { useSetting } from './settings'
import { LiveView } from './components/LiveView'
import { StatusStrip, TabId } from './components/StatusStrip'
import { CameraTab } from './components/CameraTab'
import { AimTab } from './components/AimTab'
import { TrackTab } from './components/TrackTab'
import { RecordTab } from './components/RecordTab'
import { MoreTab } from './components/MoreTab'

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
  const [display, setDisplay, resetDisplay] = useDisplay()
  const [savedTab, setSavedTab] = useSetting<TabId>('ui.tab', 'camera')
  const [tab, setTabState] = useState<TabId>(TABS.some((x) => x.id === savedTab) ? savedTab : 'camera')
  const setTab = (t: TabId) => { setTabState(t); setSavedTab(t) }
  const [focus, setFocus] = useState<{ id: string; nonce: number } | null>(null)
  const tabRefs = useRef<Record<string, HTMLButtonElement | null>>({})
  const panelRef = useRef<HTMLElement>(null)
  useEffect(() => { panelRef.current?.scrollTo(0, 0) }, [tab])
  const cam = status?.camera

  const goto = (t: TabId) => setTab(t)
  const openMore = (id: string) => { setFocus({ id, nonce: Date.now() }); setTab('more') }
  const stopCapture = () => { api('POST', '/capture/stop').catch(() => {}) }

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
    <div className="app" data-tab={tab}>
      <a className="skip" href="#panel">Skip to controls</a>
      <header className="top">
        <img className="mark" src="/logo-mark.svg" alt="" width="26" height="26" />
        <h1>Rust<span>Flare</span></h1>
        <span className={`conn ${connected ? 'ok' : 'bad'}`}><i aria-hidden="true" />{connected ? 'Online' : 'Reconnecting…'}</span>
        {cam && <span className="camname">{cam.name} · {cam.width}×{cam.height} · {cam.bits}-bit</span>}
      </header>
      {(status?.error || (!connected && !status)) && (
        <div className="banner" role="alert">{status?.error ?? 'Connecting to the RustFlare server…'}</div>
      )}

      <section className="view" aria-label="Live view">
        <LiveView status={status} display={display} setDisplay={setDisplay} anchorPick={anchorPick} onPicked={() => setAnchorPick(false)} onDisplay={() => openMore('display')} />
        <StatusStrip status={status} goto={goto} onStop={stopCapture} hideRec={tab === 'record'} hide={tab === 'aim' ? ['sun'] : []} />
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
        {tab === 'aim' && <AimTab status={status} next={() => goto('track')} />}
        {tab === 'track' && <TrackTab status={status} anchorPick={anchorPick} setAnchorPick={setAnchorPick} goto={() => goto('aim')} next={() => goto('record')} />}
        {tab === 'record' && <RecordTab status={status} goto={(_t, f) => openMore(f)} />}
        {tab === 'more' && <MoreTab status={status} display={display} setDisplay={setDisplay} resetDisplay={resetDisplay} focus={focus} />}
      </main>
    </div>
  )
}
