import { api, Status } from '../api'
import { ErrorLine, Field, useAction } from './ui'
import { useSetting } from '../settings'

/** Flat-field capture. Lives in the Camera tab: flats belong to the optical train and the camera settings. */
export function Flats({ status }: { status: Status | null }) {
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
