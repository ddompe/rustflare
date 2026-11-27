import { useEffect, useId, useRef, useState } from 'react'
import { api, CameraInfo, Control, Status } from '../api'
import { Disclosure, ErrorLine, Section, Switch, useAction } from './ui'
import { useSetting } from '../settings'
import { Histogram } from './Histogram'
import { DeviceCard } from './DeviceCard'
import { RoiControl } from './RoiControl'
import { ColourControl } from './ColourControl'
import { FocusAssist } from './FocusAssist'
import { CoolerPanel } from './CoolerPanel'
import { CameraCatalog, useDatasheet } from '../datasheet'

const fmt = (c: Control, v: number) => {
  if (c.unit !== 'µs') return `${Math.round(v)}${c.unit ? ` ${c.unit}` : ''}`
  return v >= 1e6 ? `${(v / 1e6).toFixed(2)} s` : v >= 1000 ? `${(v / 1000).toFixed(2)} ms` : `${Math.round(v)} µs`
}

/** Exposure slider windows: a full-width drag covers only two decades, so fine control is possible. */
const EXP_RANGES = [
  { label: '10 µs – 1 ms', min: 10, max: 1_000 },
  { label: '0.1 – 10 ms', min: 100, max: 10_000 },
  { label: '1 – 100 ms', min: 1_000, max: 100_000 },
  { label: '10 ms – 1 s', min: 10_000, max: 1_000_000 },
  { label: '0.1 – 10 s', min: 100_000, max: 10_000_000 },
  { label: 'Full range', min: 0, max: Infinity },
]

/** The window in which the current value sits closest to the middle of the slider. */
function bestRange(v: number): number {
  let best = 0, bestD = Infinity
  EXP_RANGES.slice(0, -1).forEach((r, i) => {
    if (v < r.min || v > r.max) return
    const d = Math.abs(Math.log(v / r.min) / Math.log(r.max / r.min) - 0.5)
    if (d < bestD) { best = i; bestD = d }
  })
  return bestD === Infinity ? EXP_RANGES.length - 1 : best
}

function Slider({ c, marks }: { c: Control; marks?: { v: number; label: string; title?: string }[] }) {
  const id = useId()
  const isExp = c.id === 'exposure_us'
  const [range, setRange] = useState(() => (isExp ? bestRange(c.value) : 0))
  const win = isExp ? EXP_RANGES[range] : { min: 0, max: Infinity }
  const lo = Math.max(c.min, win.min), hi = Math.min(c.max, win.max)
  const toPos = (v: number) => (c.log ? Math.log(Math.min(hi, Math.max(v, lo))) : v)
  const fromPos = (p: number) => (c.log ? Math.exp(p) : p)
  const [value, setValue] = useState(c.value)
  const dragging = useRef(false)
  const timer = useRef<number>()
  const { err, run } = useAction()
  const [text, setText] = useState<string | null>(null)
  useEffect(() => { if (!dragging.current) setValue(c.value) }, [c.value])
  useEffect(() => {
    if (isExp && !dragging.current && (c.value < EXP_RANGES[range].min || c.value > EXP_RANGES[range].max) && range !== EXP_RANGES.length - 1) setRange(bestRange(c.value))
  }, [c.value]) // eslint-disable-line react-hooks/exhaustive-deps
  const send = (v: number) => { const x = Math.min(c.max, Math.max(c.min, v)); setValue(x); run(() => api('PUT', '/camera/control', { id: c.id, value: x })) }

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
      <>
        <Switch label={c.label} checked={value >= 0.5} onChange={(on) => run(() => api('PUT', '/camera/control', { id: c.id, value: on ? 1 : 0 }))} />
        <ErrorLine error={err} />
      </>
    )
  }

  const entry = (
    <div className="numwrap">
      <input
        id={`${id}-n`}
        className="num"
        inputMode="decimal"
        aria-label={`${c.label}${c.unit ? ` in ${c.unit}` : ''}, type a value`}
        value={text ?? String(Math.round(value))}
        onFocus={(e) => { setText(String(Math.round(value))); e.currentTarget.select() }}
        onChange={(e) => setText(e.target.value)}
        onBlur={() => { const v = parseFloat(text ?? ''); if (!Number.isNaN(v)) send(v); setText(null) }}
        onKeyDown={(e) => { if (e.key === 'Enter') (e.target as HTMLInputElement).blur() }}
      />
      {c.unit && <span className="unit" aria-hidden="true">{c.unit}</span>}
    </div>
  )

  return (
    <div className="ctl">
      <div className="ctl-head">
        <label htmlFor={id}>{c.label}</label>
        <output htmlFor={id} className="ctl-val">{fmt(c, value)}</output>
      </div>
      <input id={id} type="range" min={c.log ? Math.log(lo) : c.min} max={c.log ? Math.log(hi) : c.max} step={c.log ? 0.002 : c.step} value={toPos(value)} onChange={(e) => change(parseFloat(e.target.value))} />
      {marks && marks.length > 0 && (
        <div className="marks" aria-hidden="true">
          {marks.map((m) => {
            const f = (toPos(m.v) - (c.log ? Math.log(lo) : c.min)) / ((c.log ? Math.log(hi) : c.max) - (c.log ? Math.log(lo) : c.min))
            return f >= 0 && f <= 1 ? <span key={m.label} title={m.title} style={{ left: `calc(16px + (100% - 32px) * ${f})` }}><i />{m.label}</span> : null
          })}
        </div>
      )}
      {isExp ? (
        <>
          <div className="fine fine5">
            <button type="button" onClick={() => send(value / 2)} aria-label="Halve exposure">½×</button>
            <button type="button" onClick={() => send(value * 0.95)} aria-label="Decrease exposure by 5 percent">−5%</button>
            {entry}
            <button type="button" onClick={() => send(value * 1.05)} aria-label="Increase exposure by 5 percent">+5%</button>
            <button type="button" onClick={() => send(value * 2)} aria-label="Double exposure">2×</button>
          </div>
          <details className="range-more">
            <summary>Slider range: {EXP_RANGES[range]?.label}</summary>
          <div className="range-pick">
            <label htmlFor={`${id}-r`}>Slider range</label>
            <select id={`${id}-r`} value={range} onChange={(e) => setRange(parseInt(e.target.value))} title="A narrower range gives finer control">
              {EXP_RANGES.map((r, i) => <option key={r.label} value={i}>{r.label}</option>)}
            </select>
          </div>
          </details>
        </>
      ) : (
        <div className="fine fine5">
          <button type="button" onClick={() => send(value - 10)}>−10</button>
          <button type="button" onClick={() => send(value - 1)}>−1</button>
          {entry}
          <button type="button" onClick={() => send(value + 1)}>+1</button>
          <button type="button" onClick={() => send(value + 10)}>+10</button>
        </div>
      )}
      <ErrorLine error={err} />
    </div>
  )
}

const PRIMARY = ['exposure_us', 'gain']

/** Step 1: open the camera, then get a good exposure (histogram, no clipping). */
export function CameraTab({ status, next }: { status: Status | null; next: () => void }) {
  const [list, setList] = useState<CameraInfo[]>([])
  const [sel, setSel] = useSetting('camera.selected', '')
  const { err, busy, run } = useAction()
  const cam = status?.camera
  const sheet = useDatasheet('camera', status, cam ? `${cam.driver}|${cam.id}` : null)
  const cat: CameraCatalog | null | undefined = sheet.catalog

  const refresh = () => run(async () => {
    const l = await api<CameraInfo[]>('GET', '/cameras')
    setList(l)
    if (l.length && !l.some((c) => `${c.driver}|${c.id}` === sel)) setSel(`${l[0].driver}|${l[0].id}`)
  })
  useEffect(() => { refresh() }, []) // eslint-disable-line react-hooks/exhaustive-deps

  if (!cam) {
    return (
      <Section title="Open the camera" error={err}>
        <p className="lead">Pick your camera to see the Sun.</p>
        <div className="field">
          <label htmlFor="cam-pick">Camera</label>
          <select id="cam-pick" value={sel} onChange={(e) => setSel(e.target.value)}>
            {list.length === 0 && <option value="">no cameras found</option>}
            {list.map((c) => <option key={c.driver + c.id} value={`${c.driver}|${c.id}`}>{c.name} [{c.driver}]</option>)}
          </select>
        </div>
        <div className="btnrow">
          <button type="button" onClick={refresh}>Rescan</button>
          <button type="button" className="primary grow" disabled={!sel || busy} onClick={() => { const [driver, id] = sel.split('|'); run(() => api('POST', '/camera/open', { driver, id })) }}>Open camera</button>
        </div>
        {status?.startup.retrying && <p className="hint-text">Reconnecting {status.startup.camera ?? ''} {status.startup.mount ?? ''}… (mount tracking is never switched on automatically)</p>}
      </Section>
    )
  }

  const primary = PRIMARY.map((id) => cam.controls.find((c) => c.id === id)).filter((c): c is Control => !!c)
  const rest = cam.controls.filter((c) => !PRIMARY.includes(c.id))
  return (
    <>
      <Section error={err}>
        {cam.histogram.length > 0 && <Histogram bins={cam.histogram} saturated={cam.saturated} />}
        {primary.map((c) => {
          const marks = c.id === 'gain' && cat ? [cat.hcg_gain != null ? { v: cat.hcg_gain, label: 'HCG', title: cat.hcg_note ?? 'High conversion gain starts here' } : null, cat.unity_gain != null ? { v: cat.unity_gain, label: 'Unity', title: 'Unity gain' } : null].filter((m): m is { v: number; label: string; title: string } => !!m) : undefined
          return <Slider key={c.id} c={c} marks={marks} />
        })}
      </Section>
      {cam.cooler?.available && <CoolerPanel cooler={cam.cooler} />}
      {rest.length > 0 && (
        <Disclosure title="More camera controls" summary={rest.map((c) => c.label.split(' (')[0]).join(', ')}>
          {rest.map((c) => <Slider key={c.id} c={c} />)}
        </Disclosure>
      )}
      {status?.camera?.bayer && (
        <Disclosure title="Colour" summary={`white balance ${status.colour?.mode ?? ""}`}>
          <ColourControl status={status} />
        </Disclosure>
      )}
      <Disclosure title="Focus assist" summary={status?.focus?.enabled ? `score ${status.focus.lucky?.toFixed(2) ?? "…"}` : 'off'}>
        <FocusAssist status={status} />
      </Disclosure>
      <Disclosure title="Frame size" summary={status?.roi ? `${status.roi.width}×${status.roi.height}` : ''}>
        <RoiControl status={status} />
      </Disclosure>
      <DeviceCard kind="camera" data={sheet} />
      <div className="btnrow end">
        <button type="button" className="quiet" onClick={() => run(() => api('POST', '/camera/close'))}>Close camera</button>
        <button type="button" className="primary" onClick={next}>Next: Aim →</button>
      </div>
    </>
  )
}
