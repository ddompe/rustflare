import { useState } from 'react'
import { api, Status } from '../api'
import { ErrorLine, Field, useAction } from './ui'
import { useSetting } from '../settings'
import { profileOf, why } from '../profile'
import { Why } from './Profile'

/** Disc frame plus a much longer exposure for prominences, and their composite (the disc comes from the short frame). */
export function ProminencePair({ status }: { status: Status | null }) {
  const [factor, setFactor] = useSetting('capture.prom_factor', '8')
  const { err, busy, run } = useAction()
  const [ok, setOk] = useState(false)
  const idle = status?.capture.state === 'idle'
  const off = why(profileOf(status), 'prominence')
  return (
    <div>
      <p className="hint-text">Takes the disc at the current exposure, then one exposure this many times longer so the prominences show (the disc blows out), restores the exposure, and saves three PNGs: disc, long and composite. Needs the Sun in view.</p>
      <Field label="Longer exposure, times">{(id) => <input id={id} inputMode="decimal" value={factor} onChange={(e) => setFactor(e.target.value)} />}</Field>
      <button type="button" className="wide" disabled={busy || !idle || !status?.sun || !!off} aria-describedby={off ? 'why-pair' : undefined} onClick={() => run(async () => { await api('POST', '/capture/prominence', { factor: parseFloat(factor) || 8 }); setOk(true) })}>Capture prominence pair</button>
      <Why id="why-pair" text={off} subject="Prominence pair" />
      {ok && idle && <div className="hint-text" role="status">Saved: see Captures.</div>}
      <ErrorLine error={err} />
    </div>
  )
}
