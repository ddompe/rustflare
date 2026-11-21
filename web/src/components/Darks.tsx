import { api, Status } from '../api'
import { ErrorLine, Field, Switch, useAction } from './ui'
import { useSetting } from '../settings'

/** Dark frames: cap on, same exposure and gain as the Sun frames. The master dark is subtracted from the live view. */
export function Darks({ status }: { status: Status | null }) {
  const [frames, setFrames] = useSetting('capture.dark_frames', '30')
  const { err, busy, run } = useAction()
  const d = status?.darks
  const idle = status?.capture.state === 'idle'
  if (!status?.camera) return <p className="hint-text">Open a camera first.</p>
  return (
    <div>
      <Switch label="Subtract the dark in the live view" checked={!!d?.applied} disabled={!d?.available} onChange={(on) => run(() => api('PUT', '/darks/applied', { enabled: on }))}
        hint={d?.available ? `Master dark: ${d.name}. Taken at ${d.exposure_us != null ? `${(d.exposure_us / 1000).toFixed(2)} ms` : '?'}, gain ${d.gain ?? '?'}. Recordings stay raw.` : 'No master dark yet.'} />
      {d?.available && !d.matches_camera && <div className="warn" role="status">The camera's exposure or gain differs from the dark's, so it will not match. Retake the dark or change the settings back.</div>}
      <p className="hint-text">Put the cap on the telescope, keep the exposure and gain you will use on the Sun, then capture. Tracking stops while the dark is taken.</p>
      <Field label="Dark frames">{(id) => <input id={id} inputMode="numeric" value={frames} onChange={(e) => setFrames(e.target.value)} />}</Field>
      <button type="button" className="wide" disabled={busy || !idle} onClick={() => run(() => api('POST', '/capture/darks', { frames: parseInt(frames) || 30 }))}>Capture darks (cap on)</button>
      <ErrorLine error={err} />
    </div>
  )
}
