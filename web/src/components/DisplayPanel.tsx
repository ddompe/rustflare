import { useEffect, useState } from 'react'
import { api } from '../api'
import { Display } from '../display'
import { Panel, Row } from './ui'

export function DisplayPanel({ d, set, reset }: { d: Display; set: (p: Partial<Display>) => void; reset: () => void }) {
  const [stretch, setStretch] = useState(false)
  useEffect(() => { api<{ stretch: boolean }>('GET', '/preview').then((r) => setStretch(r.stretch)).catch(() => {}) }, [])
  const slider = (label: string, key: 'brightness' | 'contrast' | 'gamma', min: number, max: number) => (
    <Row label={label}>
      <input type="range" min={min} max={max} step={0.05} value={d[key]} onChange={(e) => set({ [key]: parseFloat(e.target.value) })} />
      <span className="val">{d[key].toFixed(2)}</span>
    </Row>
  )
  return (
    <Panel title="Display">
      <Row label="Auto-stretch">
        <input type="checkbox" checked={stretch} onChange={(e) => { setStretch(e.target.checked); api('PUT', '/preview', { stretch: e.target.checked }).catch(() => {}) }} />
        <span className="muted small">brightens dim 16-bit frames (everyone's preview)</span>
      </Row>
      {slider('Brightness', 'brightness', 0.3, 4)}
      {slider('Contrast', 'contrast', 0.3, 4)}
      {slider('Gamma', 'gamma', 0.3, 3)}
      <Row label="Colour">
        <select value={d.tint} onChange={(e) => set({ tint: e.target.value as Display['tint'] })}>
          <option value="none">Grayscale</option><option value="orange">H-alpha orange</option><option value="red">Deep red</option>
        </select>
        <label className="inline"><input type="checkbox" checked={d.invert} onChange={(e) => set({ invert: e.target.checked })} /> invert</label>
      </Row>
      <hr />
      <Row label="Overlays">
        <label className="inline"><input type="checkbox" checked={d.crosshair} onChange={(e) => set({ crosshair: e.target.checked })} /> crosshair</label>
        <label className="inline"><input type="checkbox" checked={d.grid} onChange={(e) => set({ grid: e.target.checked })} /> thirds</label>
        <label className="inline"><input type="checkbox" checked={d.sunOutline} onChange={(e) => set({ sunOutline: e.target.checked })} /> Sun</label>
      </Row>
      <Row label="Bullseye rings">
        <input type="range" min={0} max={8} step={1} value={d.rings} onChange={(e) => set({ rings: parseInt(e.target.value) })} />
        <span className="val">{d.rings || 'off'}</span>
      </Row>
      <Row label="Ring size">
        <input type="range" min={3} max={50} step={1} value={d.ringSize} onChange={(e) => set({ ringSize: parseInt(e.target.value) })} />
        <span className="val">{d.ringSize}%</span>
      </Row>
      <Row label="Preview fps">
        <select value={d.fps} onChange={(e) => set({ fps: parseInt(e.target.value) })}>
          {[2, 5, 10, 15, 30].map((f) => <option key={f} value={f}>{f}</option>)}
        </select>
        <span className="muted small">lower saves Wi-Fi</span>
      </Row>
      <Row>
        <span className="muted small">Wheel = zoom, drag = pan, double-click = reset ({d.zoom.toFixed(1)}×)</span>
        <button onClick={() => { reset() }}>Reset</button>
      </Row>
    </Panel>
  )
}
