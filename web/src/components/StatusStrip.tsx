import React, { useEffect, useRef, useState } from 'react'
import { api, Status, reportError } from '../api'
import { endsText, ToneIcon } from './ui'
import { recipeLabel, recordRequest, savedRecipe } from '../recipe'
import { clipping, isSun, noun, profileOf } from '../profile'
import { AFTER_MODE_NAME } from '../vocab'
import { clockNote, clockShort, sendReason } from '../mountclock'
import { Saved, sizeText } from '../saved'

export type TabId = 'camera' | 'aim' | 'track' | 'record' | 'more'

type ChipTone = 'ok' | 'warn' | 'bad' | 'info' | 'off'
/** `value` is the big numeral (readable at arm's length in sunlight); `label` says what it is. */
interface ChipSpec { key: string; tone: ChipTone; value?: string; label: string; tab: TabId; title: string }
/**
 * Something worth knowing, collected under one chip. `level` is how serious it is (the chip takes the highest), `short` leads the chip when it is
 * the only one, `word` names it in "2 notes: clock, sun".
 */
interface NoteSpec { key: string; text: string; short: string; word: string; level: 'info' | 'warn'; tab?: TabId; focus?: string; tabLabel?: string; send?: boolean; saved?: boolean }

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

/** The notes folded into one chip: a target not found yet, a mount clock that differs. (What is being imaged is said in the profile line on the Camera tab, not here.) */
export function notesFor(status: Status | null): NoteSpec[] {
  const cam = status?.camera
  if (!status || !cam) return []
  const notes: NoteSpec[] = []
  const p = profileOf(status)
  if (!status.sun && !status.tracking?.lost && !(isSun(p) || !!status.tracking)) notes.push({ key: 'sun', text: `No ${noun(p)} found yet. Too dark? Try more exposure.`, short: `No ${noun(p)} found yet`, word: noun(p), level: 'info', tab: 'camera', tabLabel: 'Camera' })
  const clock = clockNote(status)
  if (clock) notes.push({ key: 'mountclock', text: clock, short: clockShort(status) ?? 'Mount clock differs', word: 'clock', level: 'warn', tab: 'aim', focus: 'clock', tabLabel: 'Aim', send: true })
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
export function StatusStrip({ status, goto, onStop, hideRec, hide = [], noQuick, setup, saved, onSavedOpen, onSavedDismiss }: { status: Status | null; goto: (t: TabId, focus?: string) => void; onStop: () => void; hideRec?: boolean; hide?: string[]; noQuick?: boolean; /** the collapsed set-up chip, shown with the other chips */ setup?: React.ReactNode; /** what the last recording saved: a note for a few minutes, with the way to it */ saved?: Saved | null; onSavedOpen?: () => void; onSavedDismiss?: () => void }) {
  const quick = !!status?.camera && status.capture.state === 'idle' && !status.capture.paused && !noQuick
  const [qerr, setQerr] = useState('')
  const quickCall = (path: string, body?: unknown) => { setQerr(''); api('POST', path, body).catch((e: Error) => setQerr(e.message)) }
  const startQuick = () => { const r = savedRecipe(); const q = recordRequest(r, status!.camera!); quickCall(q.path, q.body) }
  const chips = chipsFor(status).filter((x) => !hide.includes(x.key))
  const notes = [
    ...notesFor(status),
    ...(saved ? [{ key: 'saved', text: `Saved ${saved.name}${saved.size != null ? ` · ${sizeText(saved.size)}` : ''}${saved.early ? ' · stopped early' : ''}.`, short: `Saved ${saved.name}`, word: 'saved', level: 'info' as const, saved: true } as NoteSpec] : []),
  ].filter((x) => !hide.includes(x.key))
  const [sheet, setSheet] = useState(false)
  const box = useRef<HTMLElement>(null)
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
  // the notes chip is as serious as its most serious note and leads with the key words ("Clock off 3 min"), not with a count
  const noteLevel: 'warn' | 'info' = notes.some((n) => n.level === 'warn') ? 'warn' : 'info'
  const noteLabel = notes.length === 1 ? notes[0].short : `${notes.length} notes: ${notes.map((n) => n.word).join(', ')}`
  return (
    <div className="strip">
      <span className="sr-only" role="status" aria-live="polite">{alerts}</span>
      {!!status?.camera && (chips.length > 0 || notes.length > 0 || !!setup) && (
        <div className="chips-wrap">
          <ul className="chips" aria-label="Warnings and notes">
            {!!setup && <li>{setup}</li>}
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
              <li ref={box as React.RefObject<HTMLLIElement>}>
                <button ref={noteBtn} type="button" className={`chip ${noteLevel} notes`} aria-expanded={sheet} aria-controls="notes-sheet" onClick={() => setSheet(!sheet)}>
                  <ToneIcon tone={noteLevel} />
                  <span className="cl">{noteLabel}</span>
                </button>
                {sheet && notes.length > 0 && (
            <div className="notes-sheet" id="notes-sheet" role="region" aria-label="Notes">
              <ul>
                {notes.map((n) => (
                  <li key={n.key}>
                    <p>{n.text}</p>
                    <div className="btnrow tight wrap">
                      {n.send && <button type="button" className="small" disabled={sending || !!sendWhy} aria-describedby={sendWhy ? 'notes-send-why' : undefined} onClick={() => { setSending(true); api('POST', '/mount/sync-site-time').catch((e) => reportError(e, 'Sending the site and time')).finally(() => setSending(false)) }}>{sending ? 'Sending…' : 'Send now'}</button>}
                      {n.saved && <button type="button" className="small" onClick={() => { setSheet(false); onSavedOpen?.() }}>Open in Files</button>}
                      {n.saved && <button type="button" className="small quiet" onClick={() => { setSheet(false); onSavedDismiss?.() }}>Dismiss</button>}
                      {n.focus && n.tab && <button type="button" className="small" onClick={() => { setSheet(false); goto(n.tab!, n.focus) }}>{n.focus ? `Show on ${n.tabLabel}` : `Open ${n.tabLabel}`}</button>}
                    </div>
                    {n.send && sendWhy && <p className="hint-text gate-why" id="notes-send-why">{sendWhy}</p>}
                  </li>
                ))}
              </ul>
            </div>
          )}
              </li>
            )}
          </ul>
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
