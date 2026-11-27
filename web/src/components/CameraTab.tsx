import { ReactNode, useEffect, useId, useRef, useState } from 'react'
import { ProfilePicker } from './Profile'
import { clipping, profileOf } from '../profile'
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

/** The marks under the gain slider. HCG and unity gain at the same gain (the ASI533) are one mark: two labels at one place print over each other. */
function gainMarks(cat: CameraCatalog): { v: number; label: string; title: string }[] {
  const hcg = cat.hcg_gain != null ? { v: cat.hcg_gain, label: 'HCG', title: cat.hcg_note ?? 'High conversion gain starts here' } : null
  const unity = cat.unity_gain != null ? { v: cat.unity_gain, label: 'Unity', title: 'Unity gain' } : null
  if (hcg && unity && hcg.v === unity.v) return [{ v: hcg.v, label: 'HCG · Unity', title: `${hcg.title} Unity gain is at the same setting.` }]
  return [hcg, unity].filter((m): m is { v: number; label: string; title: string } => !!m)
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
export function CameraTab({ status, next, setup, openOptics }: { status: Status | null; next: () => void; /** opens the Optical setups dialog */ openOptics: () => void; /** the first-session strip: below the exposure, so the first screen stays the picture, Start, profile and exposure */ setup?: ReactNode }) {
  const [list, setList] = useState<CameraInfo[]>([])
  const [sel, setSel] = useSetting('camera.selected', '')
  const { err, busy, run } = useAction()
  const cam = status?.camera
  const sheet = useDatasheet('camera', status, cam ? `${cam.driver}|${cam.id}` : null)
  const cat: CameraCatalog | null | undefined = sheet.catalog
  const [askClose, setAskClose] = useState(false)
  // what closing the camera would cut short: said before it happens
  const [running, setRunning] = useState<string[]>([])
  const whatRuns = (s: Status | null) => [
    ...(s && s.capture.state !== 'idle' ? ['the recording (what is already written stays on the drive)'] : []),
    ...(s?.guiding.state === 'calibrating' ? ['the calibration'] : []),
    ...(s?.guiding.state === 'guiding' ? ['guiding (the mount keeps tracking, but is no longer corrected)'] : []),
  ]
  // asked of the server, not of the last picture of its state: the answer must be the truth at the moment of the tap
  // the engine only closes an idle camera: end what runs, wait until it has, then close
  const closeNow = async () => {
    await api('POST', '/guiding/stop').catch(() => {})
    await api('POST', '/capture/stop').catch(() => {})
    for (let i = 0; i < 40; i++) { const s = await api<Status>('GET', '/status'); if (s.capture.state === 'idle') break; await new Promise((r) => setTimeout(r, 250)) }
    await api('POST', '/camera/close')
  }
  const askClosing = () => run(async () => {
    const list = whatRuns(await api<Status>('GET', '/status'))
    if (list.length) { setRunning(list); setAskClose(true) } else await api('POST', '/camera/close')
  })

  const refresh = () => run(async () => {
    const l = await api<CameraInfo[]>('GET', '/cameras')
    setList(l)
    if (l.length && !l.some((c) => `${c.driver}|${c.id}` === sel)) setSel(`${l[0].driver}|${l[0].id}`)
  })
  useEffect(() => { refresh() }, []) // eslint-disable-line react-hooks/exhaustive-deps

  if (!cam) {
    return (
      <Section title="Open the camera" error={err}>
        {setup}
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
        <h2 className="sr-only">What are you imaging?</h2>
        <ProfilePicker status={status} />
      </Section>
    )
  }

  const primary = PRIMARY.map((id) => cam.controls.find((c) => c.id === id)).filter((c): c is Control => !!c)
  const rest = cam.controls.filter((c) => !PRIMARY.includes(c.id))
  return (
    <>
      {setup}
      <h2 className="sr-only">What are you imaging?</h2>
      <ProfilePicker status={status} />
      <Section error={err}>
        {status?.optics?.setup?.active && <button type="button" className="setup-line quiet" aria-label={`Optical setup, in use: ${status.optics.setup.active}. Opens the optical setups.`} onClick={openOptics}>In use: {status.optics.setup.active}{status.optics.setup.modified ? ' (modified)' : ''}{status.optics.scale_arcsec_per_px ? ` · ${status.optics.scale_arcsec_per_px.toFixed(2)}″/px` : ''}</button>}
        {cam.histogram.length > 0 && <Histogram bins={cam.histogram} saturated={cam.saturated} rgb={cam.histogram_rgb} saturatedRgb={cam.saturated_rgb} planetClip={profileOf(status) === 'planet' ? clipping(cam, 'planet') : null} />}
        {primary.map((c) => {
          const marks = c.id === 'gain' && cat ? gainMarks(cat) : undefined
          return <Slider key={c.id} c={c} marks={marks} />
        })}
      </Section>
      {cam.cooler?.available && <CoolerPanel cooler={cam.cooler} />}
      {rest.length > 0 && (
        <Disclosure title="Extra controls" summary={rest.map((c) => c.label.split(' (')[0]).join(', ')}>
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
      {askClose && (
        <div className="confirm" role="alertdialog" aria-label="Close the camera">
          <p>Closing the camera ends {running.join(' and ')}.</p>
          <div className="btnrow tight">
            <button type="button" className="grow" onClick={() => setAskClose(false)}>Keep the camera open</button>
            <button type="button" className="danger grow" onClick={() => { setAskClose(false); run(closeNow) }}>Close the camera</button>
          </div>
        </div>
      )}
      <div className="btnrow end">
        <button type="button" className="quiet" onClick={askClosing}>Close camera</button>
        <button type="button" className="primary" onClick={next}>Next: Aim →</button>
      </div>
    </>
  )
}
