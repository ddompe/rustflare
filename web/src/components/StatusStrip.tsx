import { useEffect, useRef, useState } from 'react'
import { api, Status, reportError } from '../api'
import { endsText, ToneIcon } from './ui'
import { recipeLabel, recordRequest, savedRecipe } from '../recipe'
import { clipping, isSun, noun, profileOf, PROFILES } from '../profile'
import { AFTER_MODE_NAME } from '../vocab'
import { clockNote, sendReason } from '../mountclock'

export type TabId = 'camera' | 'aim' | 'track' | 'record' | 'more'

type ChipTone = 'ok' | 'warn' | 'bad' | 'info' | 'off'
/** `value` is the big numeral (readable at arm's length in sunlight); `label` says what it is. */
interface ChipSpec { key: string; tone: ChipTone; value?: string; label: string; tab: TabId; title: string }
/** Something worth knowing that is not a problem: collected under one "N notes" chip. */
interface NoteSpec { key: string; text: string; tab?: TabId; focus?: string; tabLabel?: string; send?: boolean }

/** Warnings that matter, kept visible as chips that jump to the place that fixes them (clipping, no target, parked, calibrating). */
export function chipsFor(status: Status | null): ChipSpec[] {
  const cam = status?.camera
  if (!status || !cam) return []
  const chips: ChipSpec[] = []
  const g = status.guiding
  const clip = clipping(cam, profileOf(status))
  if (clip.clipped) chips.push({ key: 'clip', tone: 'bad', value: clip.value, label: clip.label, tab: 'camera', title: clip.help })
  // not finding the Sun is a problem; not finding a planet before anything is aimed or tracked is only a fact
  if (!status.sun && !status.tracking?.lost && (isSun(profileOf(status)) || !!status.tracking)) chips.push({ key: 'sun', tone: 'warn', label: `No ${noun(profileOf(status))} found`, tab: 'camera', title: 'Too dark? Try more exposure' })
  if (status.mount?.park === 'parked' || status.mount?.park === 'parking') chips.push({ key: 'park', tone: 'warn', label: 'Mount parked', tab: 'aim', title: 'Parked on the mount: motion is disabled' })
  // a lost target is shown once, in the status bar at the bottom
  if (g.state === 'calibrating') chips.push({ key: 'trk', tone: 'warn', label: 'Calibrating', tab: 'track', title: 'The mount is moving the picture while it calibrates' })
  return chips
}

/** The informational chips folded into one: what is being imaged, a target not found yet, a mount clock that differs. */
export function notesFor(status: Status | null): NoteSpec[] {
  const cam = status?.camera
  if (!status || !cam) return []
  const notes: NoteSpec[] = []
  const p = profileOf(status)
  if (!isSun(p)) notes.push({ key: 'profile', text: `Imaging: ${PROFILES.find((x) => x.id === p)?.label ?? ''}. Change it on the Camera tab.`, tab: 'camera', tabLabel: 'Camera' })
  if (!status.sun && !status.tracking?.lost && !(isSun(p) || !!status.tracking)) notes.push({ key: 'sun', text: `No ${noun(p)} found yet. Too dark? Try more exposure.`, tab: 'camera', tabLabel: 'Camera' })
  const clock = clockNote(status)
  if (clock) notes.push({ key: 'mountclock', text: clock, tab: 'aim', focus: 'clock', tabLabel: 'Aim', send: true })
  return notes
}

/** How big the quick recording is, so one tap's consequence is visible: "· 100 frames" or "· 30 s". */
const quickSize = (r: ReturnType<typeof savedRecipe>) => (r.kind === 'hdr' ? '' : r.frames?.trim() ? ` · ${r.frames.trim()} frames` : r.seconds?.trim() ? ` · ${r.seconds.trim()} s` : '')

/** What happens to the frames, said where you press Start: the after-burst mode (same names as the cards), and a stacker that is not there. */
function keepText(status: Status): { text: string; warn: boolean } {
  const a = status.after_burst
  if (!a || a.mode === 'video') return { text: AFTER_MODE_NAME.video, warn: false }
  if (!a.stacker_connected) return { text: 'Stacker off, saves video', warn: true }
  return { text: AFTER_MODE_NAME[a.mode], warn: a.mode === 'stack_only' }
}

/** The one Start button: always visible when the camera is open and idle, and says what one tap will do. */
export function StatusStrip({ status, goto, onStop, hideRec, hide = [], noQuick }: { status: Status | null; goto: (t: TabId, focus?: string) => void; onStop: () => void; hideRec?: boolean; hide?: string[]; noQuick?: boolean }) {
  const quick = !!status?.camera && status.capture.state === 'idle' && !status.capture.paused && !noQuick
  const [qerr, setQerr] = useState('')
  const quickCall = (path: string, body?: unknown) => { setQerr(''); api('POST', path, body).catch((e: Error) => setQerr(e.message)) }
  const startQuick = () => { const r = savedRecipe(); const q = recordRequest(r, status!.camera!); quickCall(q.path, q.body) }
  const chips = chipsFor(status).filter((x) => !hide.includes(x.key))
  const notes = notesFor(status).filter((x) => !hide.includes(x.key))
  const [sheet, setSheet] = useState(false)
  const box = useRef<HTMLDivElement>(null)
  const noteBtn = useRef<HTMLButtonElement>(null)
  useEffect(() => {
    if (!sheet) return
    const away = (e: Event) => { if (!box.current?.contains(e.target as Node)) setSheet(false) }
    const esc = (e: KeyboardEvent) => { if (e.key === 'Escape') { setSheet(false); noteBtn.current?.focus() } }
    document.addEventListener('pointerdown', away)
    document.addEventListener('keydown', esc)
    return () => { document.removeEventListener('pointerdown', away); document.removeEventListener('keydown', esc) }
  }, [sheet])
  useEffect(() => { if (sheet && notes.length === 0) setSheet(false) }, [sheet, notes.length])
  const [sending, setSending] = useState(false)
  const c = status?.capture
  const recording = !!c && c.state !== 'idle' && !hideRec
  const alerts = chips.filter((x) => x.tone === 'bad').map((x) => `${x.value ?? ''} ${x.label}`.trim()).join('. ')
  const calibrating = status?.guiding.state === 'calibrating'
  const keep = quick ? keepText(status!) : null
  const sendWhy = status ? sendReason(status) : null
  return (
    <div className="strip">
      <span className="sr-only" role="status" aria-live="polite">{alerts}</span>
      {!!status?.camera && (chips.length > 0 || notes.length > 0) && (
        <div className="chips-wrap" ref={box}>
          <ul className="chips" aria-label="Warnings and notes">
            {chips.map((x) => (
              <li key={x.key}>
                <button type="button" className={`chip ${x.tone} ${x.key}`} title={x.title} onClick={() => goto(x.tab)}>
                  <ToneIcon tone={x.tone} />
                  {x.value && <span className="cv">{x.value}</span>}
                  <span className="cl">{x.label}</span>
                </button>
              </li>
            ))}
            {notes.length > 0 && (
              <li>
                <button ref={noteBtn} type="button" className="chip info notes" aria-expanded={sheet} aria-controls="notes-sheet" onClick={() => setSheet(!sheet)}>
                  <ToneIcon tone="info" />
                  <span className="cl">{notes.length} {notes.length === 1 ? 'note' : 'notes'}</span>
                </button>
              </li>
            )}
          </ul>
          {sheet && notes.length > 0 && (
            <div className="notes-sheet" id="notes-sheet" role="region" aria-label="Notes">
              <ul>
                {notes.map((n) => (
                  <li key={n.key}>
                    <p>{n.text}</p>
                    <div className="btnrow tight wrap">
                      {n.send && <button type="button" className="small" disabled={sending || !!sendWhy} aria-describedby={sendWhy ? 'notes-send-why' : undefined} onClick={() => { setSending(true); api('POST', '/mount/sync-site-time').catch((e) => reportError(e, 'Sending the site and time')).finally(() => setSending(false)) }}>{sending ? 'Sending…' : 'Send now'}</button>}
                      {n.focus && n.tab && <button type="button" className="small" onClick={() => { setSheet(false); goto(n.tab!, n.focus) }}>{n.focus ? `Show on ${n.tabLabel}` : `Open ${n.tabLabel}`}</button>}
                    </div>
                    {n.send && sendWhy && <p className="hint-text gate-why" id="notes-send-why">{sendWhy}</p>}
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}
      {qerr && <div className="err" role="alert">{qerr}</div>}
      {quick && keep && (
        <div className="quick" role="group" aria-label="Quick capture">
          <button type="button" className={`quick-rec ${keep.warn && !calibrating ? 'warn' : ''}`} disabled={calibrating} aria-describedby={calibrating ? 'start-why' : undefined} onClick={startQuick}>
            <span className="qr-main"><svg className="qr-play" viewBox="0 0 12 12" width="12" height="12" aria-hidden="true"><path d="M2.5 1.5v9l8-4.5z" fill="currentColor" /></svg>Start <span className="muted">{recipeLabel(savedRecipe())}{quickSize(savedRecipe())}</span></span>
            <span className="qr-keep" id={calibrating ? 'start-why' : undefined}>{calibrating ? 'Calibrating: the mount is moving the picture' : keep.text}</span>
          </button>
          <button type="button" onClick={() => quickCall('/capture/single', {})}>Snap</button>
        </div>
      )}
      {!recording && c?.paused && !hideRec && (
        <div className="recbar" role="status">
          <div className="rec-text"><b>Recording paused</b> <span className="muted rec-sub">{c.paused_reason ?? ''}</span></div>
          {c.paused_reason === 'paused by you' && <button type="button" className="quick-rec" onClick={() => api('POST', '/capture/resume').catch((e) => reportError(e, 'Resume'))}>Resume</button>}
          <button type="button" className="danger" onClick={onStop}>Discard</button>
        </div>
      )}
      {recording && c && (
        <div className="recbar">
          <div className="rec-text">
            <span className="rec-dot" aria-hidden="true" />
            <b>{c.state === 'flushing' ? 'Saving' : c.state === 'flats' ? 'Flats' : 'Recording'}</b>
            <span className="cv">{c.done}{c.total != null ? `/${c.total}` : ''}</span><span className="cl">frames</span>
            <span className="muted rec-sub">
              {c.burst != null && `burst ${c.burst}${c.bursts_total ? `/${c.bursts_total}` : ''}`}{c.next_in_s != null ? ` · next in ${c.next_in_s.toFixed(0)} s` : ''}{c.eta_s != null && c.eta_s > 0 ? ` · ends ${endsText(c.eta_s)}` : ''}
            </span>
            {c.total != null && c.state !== 'flushing' && <progress max={c.total} value={c.done} aria-label="Frames captured" />}
          </div>
          {['sequence', 'bursts'].includes(c.state) && <button type="button" onClick={() => api('POST', '/capture/pause').catch((e) => reportError(e, 'Pause'))}>Pause</button>}
          {['single', 'sequence', 'bursts', 'flats'].includes(c.state) && <button type="button" className="danger" onClick={onStop}>Stop</button>}
        </div>
      )}
    </div>
  )
}
