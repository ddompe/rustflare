import { api, Status } from '../api'
import { ErrorLine, Field, Segmented, useAction } from './ui'

/** White balance of a one-shot-colour camera. It colours the live view and the single-frame PNG; recordings stay raw. */
export function ColourControl({ status }: { status: Status | null }) {
  const { err, run } = useAction()
  const cam = status?.camera
  const c = status?.colour
  if (!cam?.bayer || !c) return null
  const set = (mode: string, r = c.r, b = c.b) => run(() => api('PUT', '/colour', { mode, r, b }))
  const g = c.applied
  return (
    <div>
      <Segmented label="White balance" value={c.mode as 'off' | 'auto' | 'manual'} onChange={(m) => set(m)} options={[{ id: 'auto', label: 'Auto' }, { id: 'manual', label: 'Manual' }, { id: 'off', label: 'Off' }]} />
      <p className="hint-text">
        {c.mode === 'auto' && 'Makes the bright part of the picture neutral (the Sun is taken as white).'}
        {c.mode === 'manual' && 'Red and blue gains are set by you (green is 1).'}
        {c.mode === 'off' && 'The sensor’s own colours, no balance.'}
        {' '}Now: red ×{g[0].toFixed(2)}, blue ×{g[2].toFixed(2)}. Applies to the live view and the colour PNG of a single frame; SER and FITS recordings are always the raw {cam.bayer} mosaic.
      </p>
      {c.mode === 'manual' && (
        <div className="grid2">
          <Field label="Red gain">{(id) => <input id={id} type="number" min="0.25" max="4" step="0.05" value={c.r} onChange={(e) => set('manual', parseFloat(e.target.value) || 1, c.b)} />}</Field>
          <Field label="Blue gain">{(id) => <input id={id} type="number" min="0.25" max="4" step="0.05" value={c.b} onChange={(e) => set('manual', c.r, parseFloat(e.target.value) || 1)} />}</Field>
        </div>
      )}
      <ErrorLine error={err} />
    </div>
  )
}
