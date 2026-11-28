import { useState } from 'react'
import { api, Status } from '../api'
import { ErrorLine, Field, Switch, useAction } from './ui'
import { useSetting } from '../settings'

/** Flat-field capture. Lives in the Camera tab: flats belong to the optical train and the camera settings. */
export function Flats({ status }: { status: Status | null }) {
  const [name, setName] = useSetting('capture.flat_name', '')
  const [flatFrames, setFlatFrames] = useSetting('capture.flat_frames', '50')
  const [level, setLevel] = useSetting('capture.flat_level', '50')
  const { err, run } = useAction()
  const [step, setStep] = useState<'idle' | 'cap'>('idle')
  const busy = !!status && status.capture.state !== 'idle'
  const n = (s: string) => (s.trim() === '' ? undefined : parseFloat(s))
  if (!status?.camera) return <p className="hint-text">Open a camera first.</p>
  return (
    <div>
      <Switch label="Apply flat to the live view" checked={!!status.flats?.applied} disabled={!status.flats?.available} onChange={(on) => run(() => api('PUT', '/flats/applied', { enabled: on }))} hint={status.flats?.available ? `Master flat: ${status.flats.name}. Recordings stay raw.` : 'No master flat yet: capture flats below.'} />
      {status.flats?.available && (status.flats.differs?.length ?? 0) > 0 && <div className="warn" role="status">This flat was taken with a different {status.flats.differs.join(', ')}, so it will not correct this picture. Capture a new flat.</div>}
      {status.capture.state === 'flats' ? (
        <div className="confirm" role="status"><p>Capturing flats: {status.capture.phase}</p><div className="hint-text">Keep the cap or diffuser on until this finishes. Exposure is adjusted automatically and restored afterwards.</div></div>
      ) : step === 'idle' ? (
        <>
          <div className="grid2">
            <Field label="Flat frames">{(id) => <input id={id} inputMode="numeric" value={flatFrames} onChange={(e) => setFlatFrames(e.target.value)} />}</Field>
            <Field label="Target level (%)">{(id) => <input id={id} inputMode="numeric" value={level} onChange={(e) => setLevel(e.target.value)} />}</Field>
          </div>
          <Field label="Name (optional)">{(id) => <input id={id} value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. flats-2026-11" />}</Field>
          <button type="button" className="wide" disabled={busy} onClick={() => setStep('cap')}>Start flats…</button>
        </>
      ) : (
        <div className="confirm" role="alertdialog" aria-label="Flats">
          <p>1. Put the flat cap or diffuser on the telescope.<br />2. Check the picture is evenly lit (no Sun disc). Current level: <b>{Math.round(status.camera.mean * 100)}%</b>; the target is {level}% and the exposure is adjusted for you.<br />3. Press Capture. Tracking is stopped while flats are taken.</p>
          <div className="btnrow tight">
            <button type="button" className="grow" onClick={() => setStep('idle')}>Cancel</button>
            <button type="button" className="primary grow" disabled={busy} onClick={() => run(async () => { await api('POST', '/capture/flats', { name: name.trim() || undefined, frames: n(flatFrames), level: (n(level) ?? 50) / 100 }); setStep('idle') })}>Capture</button>
          </div>
        </div>
      )}
      <ErrorLine error={err} />
    </div>
  )
}
