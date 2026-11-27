import { useEffect, useRef, useState } from 'react'
import { api, Cooler } from '../api'
import { ErrorLine, RangeField, Section, Switch, useAction } from './ui'

type Sample = { t: number; temp: number | null; power: number | null }

/** Sensor temperature (line) and cooler power (dashed grey line, 0 to 100) over the last few minutes. */
function History({ samples }: { samples: Sample[] }) {
  const W = 300, H = 70, PAD = 4
  const pts = samples.filter((s) => s.temp != null)
  if (pts.length < 2) return <div className="focus-graph small empty" role="img" aria-label="Temperature history: collecting">Collecting…</div>
  const t1 = pts[pts.length - 1].t, t0 = pts[0].t
  const temps = pts.map((s) => s.temp as number)
  let lo = Math.min(...temps), hi = Math.max(...temps)
  if (hi - lo < 2) { lo -= 1; hi += 1 }
  const x = (t: number) => PAD + ((t - t0) / Math.max(1, t1 - t0)) * (W - 2 * PAD)
  const y = (v: number) => H - PAD - ((v - lo) / (hi - lo)) * (H - 2 * PAD)
  const line = pts.map((s) => `${x(s.t).toFixed(1)},${y(s.temp as number).toFixed(1)}`).join(' ')
  const pw = samples.filter((s) => s.power != null).map((s) => `${x(s.t).toFixed(1)},${(H - PAD - ((s.power as number) / 100) * (H - 2 * PAD)).toFixed(1)}`).join(' ')
  const last = pts[pts.length - 1].temp as number
  return (
    <svg className="focus-graph small" viewBox={`0 0 ${W} ${H}`} role="img" aria-label={`Sensor temperature history, now ${last.toFixed(1)} degrees, from ${lo.toFixed(0)} to ${hi.toFixed(0)}`}>
      {pw && <polyline points={pw} fill="none" stroke="var(--muted)" strokeWidth="1.5" strokeDasharray="3 3" vectorEffect="non-scaling-stroke" />}
      <polyline points={line} className="fg-line" fill="none" />
    </svg>
  )
}

/** Cooler of a cooled camera: switch, target temperature, power and sensor temperature, fan and anti-dew where the camera has them. */
export function CoolerPanel({ cooler }: { cooler: Cooler }) {
  const { err, run } = useAction()
  const hist = useRef<Sample[]>([])
  const [, tick] = useState(0)
  const lastPush = useRef(0)
  const lo = cooler.target_min_c ?? -30, hi = cooler.target_max_c ?? 30
  const [target, setTarget] = useState(cooler.target_c ?? Math.min(hi, Math.max(lo, 0)))
  const dragging = useRef(false)
  const timer = useRef<number>()
  useEffect(() => { if (!dragging.current && cooler.target_c != null) setTarget(cooler.target_c) }, [cooler.target_c])
  useEffect(() => {
    const now = Date.now() / 1000
    if (now - lastPush.current < 2.5) return
    lastPush.current = now
    hist.current = [...hist.current, { t: now, temp: cooler.temp_c, power: cooler.power_pct }].filter((s) => s.t > now - 600)
    tick((n) => n + 1)
  }, [cooler.temp_c, cooler.power_pct])

  const put = (body: object) => run(() => api('PUT', '/camera/cooler', body))
  const changeTarget = (v: number) => {
    setTarget(v)
    dragging.current = true
    window.clearTimeout(timer.current)
    timer.current = window.setTimeout(() => { put({ target_c: v }).finally(() => { dragging.current = false }) }, 300)
  }
  const cold = cooler.temp_c != null && cooler.temp_c < 15

  return (
    <Section title="Cooler">
      <Switch
        label="Cooler"
        checked={cooler.on}
        onChange={(on) => put({ on })}
        hint={cooler.warming_up ? 'Warming the sensor before switching off…' : cooler.on ? 'Holding the sensor at the target.' : 'Off. Switching on cools the sensor towards the target.'}
      />
      <RangeField label="Target temperature" value={target} min={lo} max={hi} step={1} show={`${Math.round(target)} °C`} onChange={changeTarget} />
      <dl className="specs">
        <div><dt>Sensor</dt><dd data-testid="cooler-temp">{cooler.temp_c != null ? `${cooler.temp_c.toFixed(1)} °C` : 'n/a'}</dd></div>
        {cooler.power_pct != null && <div><dt>Cooler power</dt><dd data-testid="cooler-power">{Math.round(cooler.power_pct)} %</dd></div>}
      </dl>
      <History samples={hist.current} />
      {cooler.fan != null && <Switch label="Fan" checked={cooler.fan} onChange={(fan) => put({ fan })} />}
      {cooler.heater != null && <Switch label="Anti-dew heater" checked={cooler.heater} onChange={(heater) => put({ heater })} hint="Warms the window so it does not mist up." />}
      <p className="hint-text">
        Cooling a sensor 30 °C or more below the air takes a few minutes, and warming it up too fast stresses it and fogs the window. Switching the cooler off while the sensor is cold therefore raises the target 1 °C every 15 s first (about 4 °C a minute) and switches off when the sensor is near room temperature.
        Cooling does little for short planet or Sun exposures (a few milliseconds), so you may leave it off for those.
      </p>
      {cooler.warming_up && <div className="btnrow"><button type="button" onClick={() => put({ on: false, immediate: true })} title="Cut the cooler now, without the gentle warm-up">Switch off now</button></div>}
      {cold && !cooler.on && !cooler.warming_up && <p className="hint-text">The sensor is still cold: let it reach room temperature before opening the case or covering it, or the window will mist.</p>}
      <ErrorLine error={err} />
    </Section>
  )
}
