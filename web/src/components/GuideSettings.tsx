import { useEffect, useId, useState } from 'react'
import { api, Status } from '../api'
import { ErrorLine, useAction } from './ui'

type P = NonNullable<Status['guide_params']>

function Num({ label, hint, value, min, max, step, unit, onChange }: { label: string; hint: string; value: number; min: number; max: number; step: number; unit: string; onChange: (v: number) => void }) {
  const id = useId()
  return (
    <div className="ctl">
      <div className="ctl-head"><label htmlFor={id}>{label}</label><output htmlFor={id} className="ctl-val">{value}{unit}</output></div>
      <input id={id} type="range" min={min} max={max} step={step} value={value} onChange={(e) => onChange(parseFloat(e.target.value))} />
      <div className="hint-text">{hint}</div>
    </div>
  )
}

/** Guiding tuning. The defaults correct slow drift only; seeing is averaged away, not chased. Applies the next time guiding starts. */
export function GuideSettings({ status }: { status: Status | null }) {
  const cur = status?.guide_params
  const [p, setP] = useState<P | null>(cur ?? null)
  const { err, busy, run } = useAction()
  useEffect(() => { if (cur && !p) setP(cur) }, [cur]) // eslint-disable-line react-hooks/exhaustive-deps
  if (!p) return null
  const set = (k: keyof P) => (v: number) => setP({ ...p, [k]: v })
  return (
    <div>
      <Num label="Averaging window" hint="Seeing is averaged over this long before judging the position." value={p.window_s} min={2} max={60} step={1} unit=" s" onChange={set('window_s')} />
      <Num label="Dead band" hint="Averaged offsets smaller than this are left alone." value={p.deadband_px} min={0} max={30} step={0.5} unit=" px" onChange={set('deadband_px')} />
      <Num label="Correction per pulse" hint="Fraction of the error corrected each time; below 100 % never over-corrects." value={Math.round(p.gain * 100)} min={5} max={100} step={5} unit=" %" onChange={(v) => set('gain')(v / 100)} />
      <Num label="Guide rate" hint="Pulse speed, multiples of sidereal (the AM3 accepts 0.1 to 0.9)." value={p.speed} min={0.1} max={0.9} step={0.05} unit="×" onChange={set('speed')} />
      <Num label="Fast rate" hint="Used only while the error is above the threshold below (after a manual slew)." value={p.fast_speed} min={0.1} max={0.9} step={0.05} unit="×" onChange={set('fast_speed')} />
      <Num label="Fast above" hint="Errors larger than this use the fast rate." value={p.fast_above_px} min={2} max={200} step={1} unit=" px" onChange={set('fast_above_px')} />
      <div className="btnrow tight">
        <button type="button" className="grow" disabled={busy} onClick={() => run(async () => { await api('PUT', '/guiding/params', p) })}>Save (applies when guiding starts)</button>
      </div>
      <ErrorLine error={err} />
    </div>
  )
}
