import { useEffect, useState } from 'react'
import { api, reportError } from '../api'
import { Disclosure, ErrorLine, Field, Segmented, Switch, useAction } from './ui'

export interface StackEntry {
  source: string
  output: string
  path: string
  thumb: string
  mtime: number
  frames: number
  kept: number
  signal: number
  aps?: number
  notes: string[]
  exposure_us: number | null
  dark_applied: boolean
  flat_applied: boolean
}
export interface StackConfig {
  enabled: boolean
  policy: 'when_idle' | 'between_bursts' | 'always'
  min_gap_s: number
  keep_percent: number
  multipoint: boolean
  threads: number
  float: boolean
  flat: string | null
  darks: string[]
  include_unmarked: boolean
  disc_mask: boolean
  disc_scale: number
}
interface Calibration { path: string; kind: 'dark' | 'flat'; frames: number; exposure_us: number | null; gain: number | null }
export interface StackInfo {
  service: string
  restart?: { attempts: number; error: string }
  status: { state: string; reason: string; current: string | null; pending: number; done: number; last_error: string | null } | null
  config: StackConfig
  stacks: StackEntry[]
  total: number
  calibration: Calibration[]
}

/** The stacker's state, polled while the page is open. */
export function useStackInfo(ms = 5000): [StackInfo | null, () => void] {
  const [info, setInfo] = useState<StackInfo | null>(null)
  const [tick, setTick] = useState(0)
  useEffect(() => {
    let live = true
    const load = () => api<StackInfo>('GET', '/stack').then((i) => live && setInfo(i)).catch((e) => reportError(e, 'Reading the stacker\'s state'))
    load()
    const t = setInterval(load, ms)
    return () => { live = false; clearInterval(t) }
  }, [ms, tick])
  return [info, () => setTick((n) => n + 1)]
}

export function stackSummary(i: StackInfo | null): string {
  if (!i) return ''
  if (i.service !== 'active') return i.total ? `${i.total} stacks · not running` : 'not running'
  const s = i.status
  if (!s || !i.config.enabled) return 'switched off'
  if (s.state === 'working') return `stacking · ${s.pending} waiting`
  if (s.state === 'paused') return `paused · ${s.pending} waiting`
  return `${i.total} stacks`
}

const POLICIES = [
  { id: 'when_idle', label: 'When idle' },
  { id: 'between_bursts', label: 'Between bursts' },
  { id: 'always', label: 'Always' },
] as const
const POLICY_HINT: Record<string, string> = {
  when_idle: 'Stacks only while nothing is recording. The safest.',
  between_bursts: 'Also stacks in the gaps of a recording that waits for its next burst (when the gap is long enough). Never while frames are being written.',
  always: 'Stacks whenever there is work, even during a recording. It runs at the lowest priority, but it can still slow the writes.',
}

const label = (c: Calibration) => `${c.path.split('/').slice(-1)[0].replace('.ser', '')} · ${c.frames} frames${c.exposure_us ? ` · ${(c.exposure_us / 1000).toFixed(c.exposure_us < 10000 ? 1 : 0)} ms` : ''}${c.gain != null ? ` · gain ${c.gain}` : ''}`

export function Stacking({ info, reload, onOpenFiles }: { info: StackInfo | null; reload: () => void; onOpenFiles: () => void }) {
  const { err, busy, run } = useAction()
  const [keep, setKeep] = useState('')
  useEffect(() => { if (info) setKeep(String(info.config.keep_percent)) }, [info?.config.keep_percent]) // eslint-disable-line react-hooks/exhaustive-deps
  if (!info) return <div className="hint-text">Loading…</div>
  const c = info.config
  const save = (patch: Partial<StackConfig>) => run(async () => { await api('PUT', '/stack/config', patch); reload() })
  const s = info.status
  const flats = info.calibration.filter((x) => x.kind === 'flat')
  const darks = info.calibration.filter((x) => x.kind === 'dark')
  return (
    <div className="stacking">
      {info.service === 'active' ? (
        <div className="hint-text" role="status">
          {s?.state === 'working' ? <>Stacking <b>{s.current}</b> ({s.pending} waiting)</> : s?.state === 'paused' ? <>Paused: {s.reason}. {s.pending} waiting.</> : s?.state === 'disabled' ? 'Switched off below.' : 'Nothing to stack right now.'}
          {s?.last_error && <span className="err"> Last problem: {s.last_error}</span>}
        </div>
      ) : (
        <div className="hint-text" role="status">
          The stacker is not running{info.service === 'failed' ? ' (it stopped with an error)' : ''}. RustFlare starts it again by itself{info.restart?.attempts ? ` (tried ${info.restart.attempts} time${info.restart.attempts === 1 ? '' : 's'})` : ''}. Until it is back, bursts are saved as video.{info.restart?.error && <span className="err"> The restart failed: {info.restart.error}. Restarting the Pi brings it back.</span>}
        </div>
      )}
      <div className="hint-text">
        {info.total ? `${info.total} stacks so far.` : 'No stacks yet.'} They are saved as FITS in the <code>stacks</code> folder of the drive, by day. To look at them, preview them and download them, use the file manager.
      </div>
      <button type="button" className="wide" onClick={onOpenFiles}>Open the stacks in Files</button>
      <Disclosure title="Stacker settings" summary={`${c.keep_percent} % kept · ${c.policy === 'when_idle' ? 'when idle' : c.policy === 'always' ? 'always' : 'between bursts'}${c.enabled ? '' : ' · off'}`}>
      <Switch label="Stack finished recordings" checked={c.enabled} disabled={busy} onChange={(v) => save({ enabled: v })} />
      <Field label="When" hint={POLICY_HINT[c.policy]}>
        {() => <Segmented label="When to stack" value={c.policy} options={POLICIES.map((p) => ({ id: p.id, label: p.label }))} disabled={busy} onChange={(v) => save({ policy: v })} />}
      </Field>
      <Field label="Keep best (%): stacked from a video file" hint="The best of each recording, by sharpness. Fewer is sharper and noisier. Bursts stacked from memory use the setting under After each burst on the Record tab.">
        {(id) => <input id={id} type="number" inputMode="numeric" min={1} max={100} step={5} value={keep} onChange={(e) => setKeep(e.target.value)} onBlur={() => { const n = parseFloat(keep); if (n >= 1 && n <= 100 && n !== c.keep_percent) save({ keep_percent: n }) }} />}
      </Field>
      <Switch label="Multi-point alignment" hint="A shift for each patch of the picture, not one per frame: sharper, a little slower." checked={c.multipoint} disabled={busy} onChange={(v) => save({ multipoint: v })} />
      <Switch label="Black disc on prominence stacks" hint="Prominence lanes recorded with the coronagraph are saved raw. With this on, the stacker finds the Sun's disc in the finished stack and draws a black disc over it; off keeps the disc as it was recorded. Applies to stacks made from now on (Files has Re-stack for the others)." checked={c.disc_mask} disabled={busy} onChange={(v) => save({ disc_mask: v })} />
      {c.disc_mask && (
        <Field label="Black disc size (% of the Sun's radius)" hint="Slightly over 100 hides the bright limb ring.">
          {(id) => <input id={id} type="number" inputMode="numeric" min={90} max={120} step={1} defaultValue={Math.round(c.disc_scale * 100)} key={c.disc_scale} onBlur={(e) => { const n = parseFloat(e.target.value); if (n >= 90 && n <= 120 && Math.abs(n / 100 - c.disc_scale) > 1e-6) save({ disc_scale: n / 100 }) }} />}
        </Field>
      )}
      <Switch label="Also stack older recordings" hint="Recordings on the drive that were made before the stacker was switched on (without the finished-marker). Off: only new recordings are stacked." checked={c.include_unmarked} disabled={busy} onChange={(v) => save({ include_unmarked: v })} />
      <Field label="Flat" hint={flats.length ? 'The raw flat-field recording (Camera tab, Flats). The stacker builds its own master from it.' : 'No flat recordings on this drive yet.'}>
        {(id) => (
          <select id={id} value={c.flat ?? ''} disabled={busy} onChange={(e) => save({ flat: e.target.value || null })}>
            <option value="">None</option>
            {flats.map((f) => <option key={f.path} value={f.path}>{label(f)}</option>)}
          </select>
        )}
      </Field>
      {darks.length > 0 && (
        <Field label="Darks" hint="Tick the ones to use. Each recording gets the dark with its exposure and gain; none is applied if none matches.">
          {() => (
            <div className="checks">
              {darks.map((d) => (
                <label key={d.path} className="check">
                  <input type="checkbox" checked={c.darks.includes(d.path)} disabled={busy} onChange={(e) => save({ darks: e.target.checked ? [...c.darks, d.path] : c.darks.filter((x) => x !== d.path) })} />
                  <span>{label(d)}</span>
                </label>
              ))}
            </div>
          )}
        </Field>
      )}
        <ErrorLine error={err} />
      </Disclosure>
    </div>
  )
}
