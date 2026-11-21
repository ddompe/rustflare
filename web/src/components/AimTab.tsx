import { useEffect, useRef, useState } from 'react'
import { api, Status } from '../api'
import { Disclosure, ErrorLine, Section, Switch, useAction } from './ui'

// Speeds the ZWO AM3/AM5 supports, as multiples of the sidereal rate.
const SPEEDS = [0.25, 0.5, 1, 2, 4, 8, 20, 60, 720, 1440]

type Axis = 'primary' | 'secondary'

function MountConnect() {
  const [drivers, setDrivers] = useState<string[]>([])
  const [driver, setDriver] = useState('zwo')
  const [device, setDevice] = useState('')
  const [ports, setPorts] = useState<{ path: string; label: string }[]>([])
  const { err, busy, run } = useAction()
  useEffect(() => { api<string[]>('GET', '/mount/drivers').then(setDrivers).catch(() => {}) }, [])
  const scan = () => api<{ path: string; label: string }[]>('GET', '/mount/ports').then((p) => { setPorts(p); setDevice((d) => d || p[0]?.path || '/dev/ttyACM0') }).catch(() => {})
  useEffect(() => { scan() }, [])
  return (
    <Section title="Connect the mount" error={err}>
      <p className="lead">Connect the mount to slew, track and hold the Sun in view.</p>
      <div className="field">
        <label htmlFor="mount-driver">Mount</label>
        <select id="mount-driver" value={driver} onChange={(e) => setDriver(e.target.value)}>
          {(drivers.length ? drivers : ['zwo', 'simulator']).map((d) => <option key={d}>{d}</option>)}
        </select>
      </div>
      {driver !== 'simulator' && (
        <div className="field">
          <label htmlFor="mount-device">Serial device</label>
          <div className="btnrow tight">
            <input id="mount-device" className="grow" list="mount-ports" value={device} onChange={(e) => setDevice(e.target.value)} />
            <button type="button" onClick={scan} aria-label="Rescan serial ports">Rescan</button>
          </div>
          <datalist id="mount-ports">{ports.map((p) => <option key={p.path} value={p.path}>{p.label}</option>)}</datalist>
          {ports.length === 0 && <div className="hint-text">No serial devices found: is the mount's USB cable plugged in and the mount powered?</div>}
        </div>
      )}
      <button type="button" className="primary wide" disabled={busy} onClick={() => run(() => api('POST', '/mount/connect', { driver, device: driver === 'simulator' ? null : device }))}>Connect mount</button>
    </Section>
  )
}

/** How far the Sun is from the frame centre and which way to push (the original Centering panel, reduced to what you read at the eyepiece). */
function Framing({ status }: { status: Status }) {
  const cam = status.camera
  const sun = status.sun
  if (!cam || !sun?.partial || sun.radius <= Math.min(cam.width, cam.height) / 2) return null
  return (
    <Disclosure title="Framing a region of interest">
      <p className="hint-text">
        Only part of the disc is in view and its centre is {sun.pos.x < 0 || sun.pos.x > cam.width || sun.pos.y < 0 || sun.pos.y > cam.height ? 'outside' : 'near the edge of'} the frame. Slew the region you want into view, then go to Track and press <b>Hold position</b> (limb tracking keeps it there). "Centre and hold" moves the disc <i>centre</i> to the middle of the frame, which is only useful if you want that.
      </p>
    </Disclosure>
  )
}

function Offset({ status }: { status: Status }) {
  const cam = status.camera
  const sun = status.sun
  if (!cam) return null
  const dx = sun ? cam.width / 2 - sun.pos.x : 0 // image must move by (dx, dy) to centre the Sun
  const dy = sun ? cam.height / 2 - sun.pos.y : 0
  const dist = Math.hypot(dx, dy)
  const centered = !!sun && dist < Math.max(10, sun.radius * 0.03)
  let pad: string | null = null
  const cal = status.guiding.calibration
  if (cal && sun && !centered) {
    const p = cal.primary_dir, s = cal.secondary_dir
    const det = p.x * s.y - p.y * s.x
    if (Math.abs(det) > 1e-6) {
      const a = (s.y * dx - s.x * dy) / det
      const b = (-p.y * dx + p.x * dy) / det
      pad = `${a >= 0 ? 'E' : 'W'} and ${b >= 0 ? 'N' : 'S'}`
    }
  }
  const arrow = (d: number, pos: string, neg: string) => (Math.abs(d) < 1 ? '' : d > 0 ? pos : neg)
  return (
    <div className="offset" aria-live="polite">
      {!sun ? (
        <div className="muted">No Sun found in the frame (too dark? try more exposure).</div>
      ) : (
        <>
          <div className={`offset-big ${centered ? 'ok' : ''}`}>{centered ? 'Centred' : `${dist.toFixed(0)} px off centre`}</div>
          {!centered && (
            <div className="offset-move">
              Move the image <b>{arrow(dx, '→', '←')}{Math.abs(dx) >= 1 ? ` ${Math.abs(dx).toFixed(0)} px` : ''}</b>{' '}
              <b>{arrow(dy, '↓', '↑')}{Math.abs(dy) >= 1 ? ` ${Math.abs(dy).toFixed(0)} px` : ''}</b>
              {pad && <span className="muted"> · hold {pad}</span>}
            </div>
          )}
          <div className="hint-text">
            Sun centre {sun.pos.x.toFixed(0)}, {sun.pos.y.toFixed(0)} · radius {sun.radius.toFixed(0)} px{sun.partial ? ' (partial disc, fitted)' : ''} · {((dist / sun.radius) * 100).toFixed(0)}% of radius
          </div>
        </>
      )}
    </div>
  )
}

/** The hold-to-move slew pad: pointer down starts the slew, pointer up (or cancel, or leaving the tab) stops it. */
function Pad({ status }: { status: Status }) {
  const [speed, setSpeed] = useState(8)
  const [err, setErr] = useState<string | null>(null)
  const [active, setActive] = useState<string | null>(null)
  const held = useRef(new Map<string, Axis>())
  const speedRef = useRef(speed)
  speedRef.current = speed
  const m = status.mount

  const send = (axis: Axis, s: number) => api('POST', '/mount/slew', { axis, speed: s }).then(() => setErr(null)).catch((e: Error) => setErr(e.message))
  const start = (key: string, axis: Axis, dir: 1 | -1) => {
    if (held.current.has(key)) return
    held.current.set(key, axis)
    setActive(key)
    send(axis, dir * speedRef.current)
  }
  const stop = (key: string) => {
    const axis = held.current.get(key)
    if (!axis) return
    held.current.delete(key)
    setActive((a) => (a === key ? null : a))
    send(axis, 0)
  }
  const stopAll = () => { for (const k of Array.from(held.current.keys())) stop(k) }

  useEffect(() => {
    const keys: Record<string, [Axis, 1 | -1]> = { ArrowUp: ['secondary', 1], ArrowDown: ['secondary', -1], ArrowLeft: ['primary', -1], ArrowRight: ['primary', 1] }
    const typing = (t: EventTarget | null) => t instanceof HTMLElement && (/^(INPUT|SELECT|TEXTAREA)$/.test(t.tagName) || !!t.closest('[role=tablist]'))
    const down = (e: KeyboardEvent) => { const k = keys[e.key]; if (!k || e.repeat || typing(e.target) || e.altKey || e.ctrlKey || e.metaKey) return; e.preventDefault(); start(e.key, k[0], k[1]) }
    const up = (e: KeyboardEvent) => { if (keys[e.key]) stop(e.key) }
    window.addEventListener('keydown', down)
    window.addEventListener('keyup', up)
    window.addEventListener('blur', stopAll)
    document.addEventListener('visibilitychange', stopAll)
    return () => {
      window.removeEventListener('keydown', down)
      window.removeEventListener('keyup', up)
      window.removeEventListener('blur', stopAll)
      document.removeEventListener('visibilitychange', stopAll)
      stopAll() // never leave the mount running when this tab is left
    }
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  const btn = (key: string, label: string, name: string, axis: Axis, dir: 1 | -1) => (
    <button
      type="button"
      className={`pad-btn ${active === key ? 'held' : ''}`}
      aria-label={`${name}: hold to move`}
      aria-pressed={active === key}
      onPointerDown={(e) => { e.preventDefault(); (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId); start(key, axis, dir) }}
      onPointerUp={() => stop(key)}
      onPointerCancel={() => stop(key)}
      onLostPointerCapture={() => stop(key)}
      onContextMenu={(e) => e.preventDefault()}
      onKeyDown={(e) => { if ((e.key === ' ' || e.key === 'Enter') && !e.repeat) { e.preventDefault(); start(key, axis, dir) } }}
      onKeyUp={(e) => { if (e.key === ' ' || e.key === 'Enter') stop(key) }}
      onBlur={() => stop(key)}
    >{label}</button>
  )

  return (
    <div>
      <div className="pad" role="group" aria-label="Slew pad. Hold a direction to move, release to stop.">
        <span />{btn('ArrowUp', 'N', 'North', 'secondary', 1)}<span />
        {btn('ArrowLeft', 'W', 'West', 'primary', -1)}
        <button type="button" className="pad-stop" onClick={() => { stopAll(); api('POST', '/mount/stop').catch((e: Error) => setErr(e.message)) }}>STOP</button>
        {btn('ArrowRight', 'E', 'East', 'primary', 1)}
        <span />{btn('ArrowDown', 'S', 'South', 'secondary', -1)}<span />
      </div>
      <div className="pad-foot">
        <label htmlFor="slew-speed">Slew speed</label>
        <select id="slew-speed" value={speed} onChange={(e) => setSpeed(parseFloat(e.target.value))} disabled={!m}>
          {SPEEDS.map((s) => <option key={s} value={s}>{s}× sidereal</option>)}
        </select>
      </div>
      <div className="hint-text">Hold a direction to move, release to stop. Arrow keys work too.</div>
      <ErrorLine error={err} />
    </div>
  )
}

/** Step 2: connect the mount, switch on solar tracking, slew the region of interest into view. */
export function AimTab({ status, next }: { status: Status | null; next: () => void }) {
  const { err, run } = useAction()
  const m = status?.mount
  if (!status || !m) return <MountConnect />
  return (
    <>
      <Offset status={status} />
      <Pad status={status} />
      <Section error={err}>
        <Switch label="Solar tracking" checked={m.tracking} onChange={(on) => run(() => api('POST', '/mount/tracking', { enabled: on }))} hint={m.tracking ? m.info : `${m.info}. Off: the Sun will drift out of view.`} />
      </Section>
      <Framing status={status} />
      <div className="btnrow end">
        <button type="button" className="quiet" onClick={() => run(() => api('POST', '/mount/disconnect'))}>Disconnect mount</button>
        <button type="button" className="primary" onClick={next}>Next: Track →</button>
      </div>
    </>
  )
}
