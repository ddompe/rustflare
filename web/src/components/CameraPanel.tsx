import { useEffect, useRef, useState } from 'react'
import { api, CameraInfo, Control, Status } from '../api'
import { Panel, Row, useAction } from './ui'
import { Histogram } from './Histogram'

const fmt = (c: Control, v: number) => {
  if (c.unit !== 'µs') return `${Math.round(v)}${c.unit ? ` ${c.unit}` : ''}`
  return v >= 1e6 ? `${(v / 1e6).toFixed(2)} s` : v >= 1000 ? `${(v / 1000).toFixed(2)} ms` : `${Math.round(v)} µs`
}

function Slider({ c }: { c: Control }) {
  const toPos = (v: number) => (c.log ? Math.log(Math.max(v, c.min)) : v)
  const fromPos = (p: number) => (c.log ? Math.exp(p) : p)
  const [value, setValue] = useState(c.value)
  const dragging = useRef(false)
  const timer = useRef<number>()
  const { err, run } = useAction()
  const [text, setText] = useState<string | null>(null)
  useEffect(() => { if (!dragging.current) setValue(c.value) }, [c.value])
  const send = (v: number) => { const x = Math.min(c.max, Math.max(c.min, v)); setValue(x); run(() => api('PUT', '/camera/control', { id: c.id, value: x })) }
  const isExp = c.id === 'exposure_us'

  const change = (p: number) => {
    let v = fromPos(p)
    v = c.step >= 1 ? Math.round(v / c.step) * c.step : v
    setValue(v)
    dragging.current = true
    window.clearTimeout(timer.current)
    timer.current = window.setTimeout(() => {
      run(() => api('PUT', '/camera/control', { id: c.id, value: v })).finally(() => { dragging.current = false })
    }, 120)
  }

  if (c.min === 0 && c.max === 1 && c.step === 1) {
    return (
      <Row label={c.label}>
        <input type="checkbox" checked={value >= 0.5} onChange={(e) => run(() => api('PUT', '/camera/control', { id: c.id, value: e.target.checked ? 1 : 0 }))} />
        {err && <span className="err">{err}</span>}
      </Row>
    )
  }
  return (
    <Row label={c.label}>
      <input type="range" min={toPos(c.min)} max={toPos(c.max)} step={c.log ? 0.01 : c.step} value={toPos(value)} onChange={(e) => change(parseFloat(e.target.value))} />
      <input
        className="num"
        value={text ?? (isExp ? String(Math.round(value)) : String(Math.round(value)))}
        onFocus={() => setText(String(Math.round(value)))}
        onChange={(e) => setText(e.target.value)}
        onBlur={() => { const v = parseFloat(text ?? ''); if (!Number.isNaN(v)) send(v); setText(null) }}
        onKeyDown={(e) => { if (e.key === 'Enter') (e.target as HTMLInputElement).blur() }}
        title={c.unit ?? undefined}
      />
      <span className="unit">{c.unit ?? ''}</span>
      {err && <span className="err">{err}</span>}
      {isExp && (
        <div className="expbar">
          <button onClick={() => send(value / 2)}>½×</button>
          <span className="muted small">{fmt(c, value)}</span>
          <button onClick={() => send(value * 2)}>2×</button>
        </div>
      )}
    </Row>
  )
}

export function CameraPanel({ status }: { status: Status | null }) {
  const [list, setList] = useState<CameraInfo[]>([])
  const [sel, setSel] = useState('')
  const { err, busy, run } = useAction()
  const cam = status?.camera

  const refresh = () => run(async () => {
    const l = await api<CameraInfo[]>('GET', '/cameras')
    setList(l)
    if (l.length && !sel) setSel(`${l[0].driver}|${l[0].id}`)
  })
  useEffect(() => { refresh() }, []) // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <Panel title="Camera" error={err}>
      <Row label="At startup">
        <label className="inline">
          <input type="checkbox" checked={status?.startup.enabled ?? true} onChange={(e) => run(() => api('PUT', '/startup', { enabled: e.target.checked }))} />
          reconnect camera &amp; mount
        </label>
      </Row>
      {status?.startup.retrying && <div className="muted small">Reconnecting {status.startup.camera ?? ''} {status.startup.mount ?? ''}… (mount tracking is never switched on automatically)</div>}
      {!cam ? (
        <Row>
          <select value={sel} onChange={(e) => setSel(e.target.value)}>
            {list.length === 0 && <option value="">no cameras found</option>}
            {list.map((c) => <option key={c.driver + c.id} value={`${c.driver}|${c.id}`}>{c.name} [{c.driver}]</option>)}
          </select>
          <button onClick={refresh}>Rescan</button>
          <button className="primary" disabled={!sel || busy} onClick={() => { const [driver, id] = sel.split('|'); run(() => api('POST', '/camera/open', { driver, id })) }}>Open</button>
        </Row>
      ) : (
        <>
          {cam.histogram.length > 0 && <Histogram bins={cam.histogram} saturated={cam.saturated} />}
          {cam.controls.map((c) => <Slider key={c.id} c={c} />)}
          <Row><button onClick={() => run(() => api('POST', '/camera/close'))}>Close camera</button></Row>
        </>
      )}
    </Panel>
  )
}
