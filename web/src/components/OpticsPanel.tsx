import { useEffect, useState } from 'react'
import { api, Status } from '../api'
import { Panel, Row, useAction } from './ui'

const arcmin = (s: number) => `${Math.floor(s / 60)}′ ${(s % 60).toFixed(0)}″`

/** Telescope focal length and pixel size → image scale → expected size of the Sun in pixels. */
export function OpticsPanel({ status }: { status: Status | null }) {
  const o = status?.optics
  const [focal, setFocal] = useState('')
  const [pixel, setPixel] = useState('')
  const [bin, setBin] = useState(1)
  const { err, run } = useAction()
  const [dirty, setDirty] = useState(false)
  useEffect(() => {
    if (!o || dirty) return
    setFocal(o.focal_length_mm?.toString() ?? '')
    setPixel(o.pixel_size_override_um?.toString() ?? '')
    setBin(o.binning)
  }, [o?.focal_length_mm, o?.pixel_size_override_um, o?.binning, dirty]) // eslint-disable-line react-hooks/exhaustive-deps
  if (!o) return null

  const num = (s: string) => (s.trim() === '' ? null : parseFloat(s))
  const sun = status?.sun
  const mismatch = sun && o.expected_radius_px && Math.abs(sun.radius / o.expected_radius_px - 1) > 0.1

  return (
    <Panel title="Optics" error={err}>
      <Row label="Focal length"><input className="num" value={focal} onChange={(e) => { setFocal(e.target.value); setDirty(true) }} placeholder="mm" /><span className="unit">mm</span></Row>
      <Row label="Pixel size">
        <input className="num" value={pixel} onChange={(e) => { setPixel(e.target.value); setDirty(true) }} placeholder={o.pixel_size_source === 'camera' ? `${o.pixel_size_um} (camera)` : 'µm'} />
        <span className="unit">µm</span>
      </Row>
      <Row label="Binning">
        <select value={bin} onChange={(e) => { setBin(parseInt(e.target.value)); setDirty(true) }}>{[1, 2, 3, 4].map((b) => <option key={b}>{b}</option>)}</select>
        <button className="primary" disabled={!dirty} onClick={() => run(async () => { await api('PUT', '/optics', { focal_length_mm: num(focal), pixel_size_um: num(pixel), binning: bin }); setDirty(false) })}>Save</button>
      </Row>
      {o.expected_radius_px ? (
        <div className="muted small">
          Image scale {o.scale_arcsec_per_px!.toFixed(2)} ″/px · the Sun is {arcmin(o.solar_radius_arcsec)} in radius today ⇒ <b>≈ {o.expected_radius_px.toFixed(0)} px</b>.
          Partial-disc tracking uses this radius.
        </div>
      ) : (
        <div className="muted small">Enter focal length (and pixel size if the camera doesn't report it) so partial-disc tracking knows how big the Sun is. Needed for an H-alpha view that doesn't show the whole disc.</div>
      )}
      {mismatch && <div className="warn small">Measured radius {sun!.radius.toFixed(0)} px differs from expected {o.expected_radius_px!.toFixed(0)} px: check focal length, reducer/barlow and binning.</div>}
    </Panel>
  )
}
