import { Status } from '../api'

export type TabId = 'camera' | 'aim' | 'track' | 'record' | 'more'

type Tone = 'ok' | 'warn' | 'bad' | 'info' | 'off'
interface ChipSpec { key: string; tone: Tone; text: string; tab: TabId; title: string }

/** Everything worth glancing at while the Sun is in view, as chips that jump to the place that fixes them. */
export function chipsFor(status: Status | null): ChipSpec[] {
  const cam = status?.camera
  if (!status || !cam) return []
  const chips: ChipSpec[] = []
  const g = status.guiding
  const t = status.tracking
  chips.push({ key: 'fps', tone: 'info', text: `${cam.fps.toFixed(1)} fps`, tab: 'camera', title: `${cam.name} · ${cam.width}×${cam.height} ${cam.bits}-bit · mean ${(cam.mean * 100).toFixed(0)}%` })
  if (cam.saturated > 0.001) chips.push({ key: 'clip', tone: 'bad', text: `Clipping ${(cam.saturated * 100).toFixed(1)}%`, tab: 'camera', title: 'Pixels at full scale: reduce exposure or gain' })
  if (!status.sun) chips.push({ key: 'sun', tone: 'warn', text: 'No Sun found', tab: 'camera', title: 'Too dark? Try more exposure' })
  if (status.mount && !status.mount.tracking) chips.push({ key: 'mount', tone: 'warn', text: 'Mount tracking off', tab: 'aim', title: status.mount.info })
  if (t?.lost) chips.push({ key: 'trk', tone: 'bad', text: 'TARGET LOST', tab: 'track', title: 'The tracked target left the frame' })
  else if (g.state === 'guiding') chips.push({ key: 'trk', tone: 'ok', text: `Holding · ${g.error_px?.toFixed(1) ?? '–'} px`, tab: 'track', title: 'Guiding error' })
  else if (g.state === 'calibrating') chips.push({ key: 'trk', tone: 'warn', text: 'Calibrating…', tab: 'track', title: 'Calibration in progress' })
  else if (t) chips.push({ key: 'trk', tone: 'info', text: `Tracking ${t.mode}`, tab: 'track', title: 'Tracking, not holding position yet' })
  if (cam.temperature_c != null) chips.push({ key: 'temp', tone: 'off', text: `${cam.temperature_c.toFixed(1)} °C`, tab: 'camera', title: 'Sensor temperature' })
  return chips
}

export function StatusStrip({ status, goto, onStop, hideRec }: { status: Status | null; goto: (t: TabId) => void; onStop: () => void; hideRec?: boolean }) {
  const chips = chipsFor(status)
  const c = status?.capture
  const recording = !!c && c.state !== 'idle' && !hideRec
  const alerts = chips.filter((x) => x.tone === 'bad').map((x) => x.text).join('. ')
  return (
    <div className="strip">
      <span className="sr-only" role="status" aria-live="polite">{alerts}</span>
      {chips.length > 0 && (
        <ul className="chips">
          {chips.map((x) => (
            <li key={x.key}>
              <button type="button" className={`chip ${x.tone} ${x.key}`} title={x.title} onClick={() => goto(x.tab)}>
                <i aria-hidden="true" />{x.text}
              </button>
            </li>
          ))}
        </ul>
      )}
      {recording && c && (
        <div className="recbar">
          <div className="rec-text">
            <span className="rec-dot" aria-hidden="true" />
            <b>{c.state === 'flushing' ? 'Saving' : c.state === 'flats' ? 'Flats' : 'Recording'}</b>
            <span className="muted">
              {c.burst != null && ` burst ${c.burst}${c.bursts_total ? `/${c.bursts_total}` : ''} ·`} {c.done}{c.total != null ? `/${c.total}` : ''} frames{c.next_in_s != null ? ` · next in ${c.next_in_s.toFixed(0)} s` : ''}
            </span>
            {c.total != null && c.state !== 'flushing' && <progress max={c.total} value={c.done} aria-label="Frames captured" />}
          </div>
          {['single', 'sequence', 'bursts', 'flats'].includes(c.state) && <button type="button" className="danger" onClick={onStop}>Stop</button>}
        </div>
      )}
    </div>
  )
}
