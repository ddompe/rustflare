import { ReactNode, useEffect, useId, useState } from 'react'
import { api, Status } from '../api'
import { Display } from '../display'
import { Disclosure, ErrorLine, Field, Switch, useAction } from './ui'
import { StorageSettings } from './Storage'
import { useSetting } from '../settings'

const arcmin = (s: number) => `${Math.floor(s / 60)}′ ${(s % 60).toFixed(0)}″`

function RangeField({ label, value, min, max, step, show, onChange }: { label: string; value: number; min: number; max: number; step: number; show: string; onChange: (v: number) => void }) {
  const id = useId()
  return (
    <div className="ctl">
      <div className="ctl-head"><label htmlFor={id}>{label}</label><output htmlFor={id} className="ctl-val">{show}</output></div>
      <input id={id} type="range" min={min} max={max} step={step} value={value} onChange={(e) => onChange(parseFloat(e.target.value))} />
    </div>
  )
}

function DisplaySettings({ d, set, reset }: { d: Display; set: (p: Partial<Display>) => void; reset: () => void }) {
  const [stretch, setStretch] = useState(false)
  useEffect(() => { api<{ stretch: boolean }>('GET', '/preview').then((r) => setStretch(r.stretch)).catch(() => {}) }, [])
  return (
    <div>
      <h3>Image</h3>
      <Switch label="Auto-stretch" checked={stretch} onChange={(on) => { setStretch(on); api('PUT', '/preview', { stretch: on }).catch(() => {}) }} hint="Brightens dim 16-bit frames (everyone's preview)." />
      <RangeField label="Brightness" value={d.brightness} min={0.3} max={4} step={0.05} show={d.brightness.toFixed(2)} onChange={(v) => set({ brightness: v })} />
      <RangeField label="Contrast" value={d.contrast} min={0.3} max={4} step={0.05} show={d.contrast.toFixed(2)} onChange={(v) => set({ contrast: v })} />
      <RangeField label="Gamma" value={d.gamma} min={0.3} max={3} step={0.05} show={d.gamma.toFixed(2)} onChange={(v) => set({ gamma: v })} />
      <Field label="Colour">
        {(id) => (
          <select id={id} value={d.tint} onChange={(e) => set({ tint: e.target.value as Display['tint'] })}>
            <option value="none">Grayscale</option><option value="orange">H-alpha orange</option><option value="red">Deep red</option>
          </select>
        )}
      </Field>
      <Switch label="Invert" checked={d.invert} onChange={(on) => set({ invert: on })} />
      <h3>Overlays</h3>
      <Switch label="Crosshair" checked={d.crosshair} onChange={(on) => set({ crosshair: on })} />
      <Switch label="Rule-of-thirds grid" checked={d.grid} onChange={(on) => set({ grid: on })} />
      <Switch label="Sun outline" checked={d.sunOutline} onChange={(on) => set({ sunOutline: on })} />
      <RangeField label="Bullseye rings" value={d.rings} min={0} max={8} step={1} show={d.rings ? String(d.rings) : 'off'} onChange={(v) => set({ rings: Math.round(v) })} />
      <RangeField label="Ring size" value={d.ringSize} min={3} max={50} step={1} show={`${d.ringSize}%`} onChange={(v) => set({ ringSize: Math.round(v) })} />
      <h3>Stream</h3>
      <Field label="Preview fps" hint="Lower saves Wi-Fi.">
        {(id) => <select id={id} value={d.fps} onChange={(e) => set({ fps: parseInt(e.target.value) })}>{[2, 5, 10, 15, 30].map((f) => <option key={f} value={f}>{f}</option>)}</select>}
      </Field>
      <p className="hint-text">Zoom: use the + and − buttons on the live view, or wheel to zoom, drag to pan, double-click to reset ({d.zoom.toFixed(1)}×).</p>
      <button type="button" className="wide" onClick={reset}>Reset display</button>
    </div>
  )
}

/** Telescope focal length and pixel size → image scale → expected size of the Sun in pixels. */
function OpticsSettings({ status }: { status: Status | null }) {
  const o = status?.optics
  const [focal, setFocal] = useState('')
  const [mult, setMult] = useState('1')
  const [pixel, setPixel] = useState('')
  const [bin, setBin] = useState(1)
  const { err, run } = useAction()
  const [dirty, setDirty] = useState(false)
  useEffect(() => {
    if (!o || dirty) return
    setFocal(o.focal_length_mm?.toString() ?? '')
    setMult(o.focal_multiplier.toString())
    setPixel(o.pixel_size_override_um?.toString() ?? '')
    setBin(o.binning)
  }, [o?.focal_length_mm, o?.focal_multiplier, o?.pixel_size_override_um, o?.binning, dirty]) // eslint-disable-line react-hooks/exhaustive-deps
  if (!o) return null

  const num = (s: string) => (s.trim() === '' ? null : parseFloat(s))
  const sun = status?.sun
  const mismatch = sun && o.expected_radius_px && Math.abs(sun.radius / o.expected_radius_px - 1) > 0.1
  return (
    <div>
      <div className="grid2">
        <Field label="Focal length (mm)">{(id) => <input id={id} inputMode="decimal" value={focal} onChange={(e) => { setFocal(e.target.value); setDirty(true) }} placeholder="telescope" />}</Field>
        <Field label="Barlow / reducer (×)">
          {(id) => (
            <>
              <input id={id} inputMode="decimal" list="barlow-presets" value={mult} onChange={(e) => { setMult(e.target.value); setDirty(true) }} title="Magnification factor: 2 for a 2x Barlow, 0.63 for a reducer, 1 for none" />
              <datalist id="barlow-presets">{[0.5, 0.63, 0.8, 1, 1.25, 1.5, 2, 2.5, 3, 4, 5].map((m) => <option key={m} value={m} />)}</datalist>
            </>
          )}
        </Field>
        <Field label="Pixel size (µm)">{(id) => <input id={id} inputMode="decimal" value={pixel} onChange={(e) => { setPixel(e.target.value); setDirty(true) }} placeholder={o.pixel_size_source === 'camera' ? `${o.pixel_size_um} (camera)` : 'µm'} />}</Field>
        <Field label="Binning">{(id) => <select id={id} value={bin} onChange={(e) => { setBin(parseInt(e.target.value)); setDirty(true) }}>{[1, 2, 3, 4].map((b) => <option key={b}>{b}</option>)}</select>}</Field>
      </div>
      {o.effective_focal_mm && Math.abs(o.focal_multiplier - 1) > 1e-6 && <div className="hint-text">Effective focal length {o.effective_focal_mm.toFixed(0)} mm.</div>}
      <button type="button" className="primary wide" disabled={!dirty} onClick={() => run(async () => { await api('PUT', '/optics', { focal_length_mm: num(focal), focal_multiplier: num(mult) ?? 1, pixel_size_um: num(pixel), binning: bin }); setDirty(false) })}>Save optics</button>
      {o.expected_radius_px ? (
        <p className="hint-text">
          Image scale {o.scale_arcsec_per_px!.toFixed(2)} ″/px · the Sun is {arcmin(o.solar_radius_arcsec)} in radius today ⇒ <b>≈ {o.expected_radius_px.toFixed(0)} px</b>. Partial-disc tracking uses this radius.
        </p>
      ) : (
        <p className="hint-text">Enter the telescope's focal length, plus the Barlow or reducer factor if you use one (and pixel size if the camera doesn't report it) so partial-disc tracking knows how big the Sun is. Needed for an H-alpha view that doesn't show the whole disc.</p>
      )}
      {mismatch && <div className="warn">Measured radius {sun!.radius.toFixed(0)} px differs from expected {o.expected_radius_px!.toFixed(0)} px: check focal length, reducer/barlow and binning.</div>}
      <ErrorLine error={err} />
    </div>
  )
}

function BufferSettings({ status }: { status: Status | null }) {
  const [bufMb, setBufMb] = useState('')
  const { err, run } = useAction()
  const c = status?.capture
  const cam = status?.camera
  const busy = !!c && c.state !== 'idle'
  const frameBytes = cam ? cam.width * cam.height * (cam.bits / 8) : 0
  const bufFrames = frameBytes ? Math.floor(((c?.buffer_mb ?? 1024) * 1048576) / frameBytes) : 0
  return (
    <div>
      <div className="btnrow tight">
        <div className="numwrap grow"><input className="num" inputMode="numeric" aria-label="RAM buffer in MB" value={bufMb || String(c?.buffer_mb ?? 1024)} onChange={(e) => setBufMb(e.target.value)} /><span className="unit" aria-hidden="true">MB</span></div>
        <button type="button" disabled={busy || !bufMb} onClick={() => run(async () => { await api('PUT', '/capture/buffer', { mb: parseFloat(bufMb) }); setBufMb('') })}>Set buffer</button>
      </div>
      <p className="hint-text">
        Frames are captured into memory and written to disk in the background. {c?.ram_available_mb ? `${c.ram_available_mb} MB of RAM free; the buffer is capped to 70% of that. ` : ''}
        {bufFrames > 0 && `Holds ≈ ${bufFrames} frames at the current size.`}
      </p>
      <ErrorLine error={err} />
    </div>
  )
}

function Flats({ status }: { status: Status | null }) {
  const [name, setName] = useSetting('capture.flat_name', '')
  const [flatFrames, setFlatFrames] = useSetting('capture.flat_frames', '50')
  const [level, setLevel] = useSetting('capture.flat_level', '50')
  const { err, run } = useAction()
  const busy = !!status && status.capture.state !== 'idle'
  const n = (s: string) => (s.trim() === '' ? undefined : parseFloat(s))
  if (!status?.camera) return <p className="hint-text">Open a camera first.</p>
  return (
    <div>
      <p className="hint-text">Preliminary: a guided "cap on, adjust, OK" workflow is planned. Capturing starts immediately: put the flat cap or diffuser on first. Exposure is adjusted automatically and restored afterwards; tracking is stopped.</p>
      <div className="grid2">
        <Field label="Flat frames">{(id) => <input id={id} inputMode="numeric" value={flatFrames} onChange={(e) => setFlatFrames(e.target.value)} />}</Field>
        <Field label="Target level (%)">{(id) => <input id={id} inputMode="numeric" value={level} onChange={(e) => setLevel(e.target.value)} />}</Field>
      </div>
      <Field label="Name (optional)">{(id) => <input id={id} value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. flats-2026-11" />}</Field>
      <button type="button" className="wide" disabled={busy} onClick={() => run(() => api('POST', '/capture/flats', { name: name.trim() || undefined, frames: n(flatFrames), level: (n(level) ?? 50) / 100 }))}>Capture flats now</button>
      <ErrorLine error={err} />
    </div>
  )
}

function Startup({ status }: { status: Status | null }) {
  const { err, run } = useAction()
  return (
    <div>
      <Switch label="Reconnect camera and mount at startup" checked={status?.startup.enabled ?? true} onChange={(on) => run(() => api('PUT', '/startup', { enabled: on }))} hint="Mount tracking is never switched on automatically." />
      {status?.startup.retrying && <div className="hint-text">Reconnecting {status.startup.camera ?? ''} {status.startup.mount ?? ''}…</div>}
      <ErrorLine error={err} />
    </div>
  )
}

/** Everything that is set once and left alone, one collapsed line each. */
export function MoreTab({ status, display, setDisplay, resetDisplay, focus }: { status: Status | null; display: Display; setDisplay: (p: Partial<Display>) => void; resetDisplay: () => void; focus: { id: string; nonce: number } | null }) {
  const [saved, setSaved] = useSetting('ui.more_open', '')
  const [open, setOpen] = useState<string | null>(focus?.id ?? (saved || null))
  useEffect(() => { if (focus) setOpen(focus.id) }, [focus?.nonce]) // eslint-disable-line react-hooks/exhaustive-deps
  const o = status?.optics
  const sun = status?.sun
  const mismatch = !!(sun && o?.expected_radius_px && Math.abs(sun.radius / o.expected_radius_px - 1) > 0.1)
  const item = (id: string, title: string, summary: string, body: ReactNode) => (
    <Disclosure key={id} title={title} summary={summary} open={open === id} onToggle={(isOpen) => { setOpen((cur) => (isOpen ? id : cur === id ? null : cur)); if (isOpen) setSaved(id); else if (open === id) setSaved('') }}>{open === id ? body : null}</Disclosure>
  )
  return (
    <>
      {item('display', 'Display', `${display.tint === 'none' ? 'grayscale' : display.tint === 'orange' ? 'H-alpha orange' : 'deep red'} · ${display.rings ? `${display.rings} rings` : 'no rings'}`, <DisplaySettings d={display} set={setDisplay} reset={resetDisplay} />)}
      {item('optics', 'Optics', mismatch ? 'check radius' : o?.focal_length_mm ? `${o.focal_length_mm}${o.focal_multiplier !== 1 ? ` × ${o.focal_multiplier}` : ''} mm${o.scale_arcsec_per_px ? ` · ${o.scale_arcsec_per_px.toFixed(2)}″/px` : ''}` : 'not set', <OpticsSettings status={status} />)}
      {item('storage', 'Storage', 'drive for captures', <StorageSettings status={status} />)}
      {item('buffer', 'RAM buffer', status?.capture ? `${status.capture.buffer_mb} MB` : '', <BufferSettings status={status} />)}
      {item('flats', 'Flats', 'rarely needed', <Flats status={status} />)}
      {item('startup', 'Startup', status?.startup.enabled ? 'reconnects' : 'manual', <Startup status={status} />)}
    </>
  )
}
