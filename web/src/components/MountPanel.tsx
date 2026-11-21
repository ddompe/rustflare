import { useEffect, useState } from 'react'
import { api, Status } from '../api'
import { Panel, Row, useAction } from './ui'

// Speeds the ZWO AM3/AM5 supports, as multiples of the sidereal rate.
const SPEEDS = [0.25, 0.5, 1, 2, 4, 8, 20, 60, 720, 1440]

export function MountPanel({ status }: { status: Status | null }) {
  const [drivers, setDrivers] = useState<string[]>([])
  const [driver, setDriver] = useState('zwo')
  const [device, setDevice] = useState('')
  const [ports, setPorts] = useState<{ path: string; label: string }[]>([])
  const [speed, setSpeed] = useState(8)
  const { err, run } = useAction()
  const m = status?.mount
  useEffect(() => { api<string[]>('GET', '/mount/drivers').then(setDrivers).catch(() => {}) }, [])
  const scan = () => api<{ path: string; label: string }[]>('GET', '/mount/ports').then((p) => { setPorts(p); setDevice((d) => d || p[0]?.path || '/dev/ttyACM0') }).catch(() => {})
  useEffect(() => { scan() }, [])

  const slew = (axis: 'primary' | 'secondary', dir: 1 | -1) => ({
    onPointerDown: (e: React.PointerEvent) => { (e.target as HTMLElement).setPointerCapture(e.pointerId); run(() => api('POST', '/mount/slew', { axis, speed: dir * speed })) },
    onPointerUp: () => run(() => api('POST', '/mount/slew', { axis, speed: 0 })),
    onPointerCancel: () => run(() => api('POST', '/mount/slew', { axis, speed: 0 })),
  })

  return (
    <Panel title="Mount" error={err}>
      {!m ? (
        <>
          <Row label="Driver">
            <select value={driver} onChange={(e) => setDriver(e.target.value)}>
              {(drivers.length ? drivers : ['zwo', 'simulator']).map((d) => <option key={d}>{d}</option>)}
            </select>
          </Row>
          {driver !== 'simulator' && (
            <Row label="Serial device">
              <input list="mount-ports" value={device} onChange={(e) => setDevice(e.target.value)} />
              <datalist id="mount-ports">{ports.map((p) => <option key={p.path} value={p.path}>{p.label}</option>)}</datalist>
              <button onClick={scan} title="Rescan serial ports">↻</button>
            </Row>
          )}
          {driver !== 'simulator' && ports.length === 0 && <div className="muted small">No serial devices found: is the mount's USB cable plugged in and the mount powered?</div>}
          <Row><button className="primary" onClick={() => run(() => api('POST', '/mount/connect', { driver, device: driver === 'simulator' ? null : device }))}>Connect</button></Row>
        </>
      ) : (
        <>
          <Row><span className="muted">{m.info}</span></Row>
          <Row label="Solar tracking">
            <input type="checkbox" checked={m.tracking} onChange={(e) => run(() => api('POST', '/mount/tracking', { enabled: e.target.checked }))} />
          </Row>
          <Row label="Slew speed">
            <select value={speed} onChange={(e) => setSpeed(parseFloat(e.target.value))}>
              {SPEEDS.map((s) => <option key={s} value={s}>{s}× sidereal</option>)}
            </select>
          </Row>
          <div className="pad">
            <span /><button {...slew('secondary', 1)}>N</button><span />
            <button {...slew('primary', -1)}>W</button>
            <button className="danger" onClick={() => run(() => api('POST', '/mount/stop'))}>STOP</button>
            <button {...slew('primary', 1)}>E</button>
            <span /><button {...slew('secondary', -1)}>S</button><span />
          </div>
          <Row><button onClick={() => run(() => api('POST', '/mount/disconnect'))}>Disconnect</button></Row>
        </>
      )}
    </Panel>
  )
}
