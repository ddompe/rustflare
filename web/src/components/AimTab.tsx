import { useEffect, useRef, useState } from 'react'
import { api, Status, reportError } from '../api'
import { Disclosure, ErrorLine, Section, Switch, useAction } from './ui'
import { useSetting } from '../settings'
import { DeviceCard } from './DeviceCard'
import { useDatasheet } from '../datasheet'
import { isSun, noun, profileForTarget, profileOf, PROFILES } from '../profile'
import { locate, SiteChoices } from './SiteHelp'
import { clockSummary, placeText, sendReason, spanWords, timeText } from '../mountclock'

// Speeds the ZWO AM3/AM5 supports, as multiples of the sidereal rate.
const SPEEDS = [0.25, 0.5, 1, 2, 4, 8, 20, 60, 720, 1440]

type Axis = 'primary' | 'secondary'

type Port = { path: string; label: string; driver?: string; model?: string }

function MountConnect() {
  const [drivers, setDrivers] = useState<string[]>([])
  const [driver, setDriver] = useSetting('mount.driver', 'zwo')
  const [device, setDevice] = useSetting('mount.device', '')
  const [ports, setPorts] = useState<Port[]>([])
  const [probing, setProbing] = useState(false)
  const [transport, setTransport] = useSetting<'usb' | 'ble'>('mount.transport', 'usb')
  const [bleAddr, setBleAddr] = useSetting('mount.ble_address', '')
  const [found, setFound] = useState<{ address: string; name: string; rssi: number | null }[] | null>(null)
  const [scanning, setScanning] = useState(false)
  const { err, busy, run } = useAction()
  const scanBle = () => run(async () => {
    setScanning(true)
    try {
      const d = await api<{ address: string; name: string; rssi: number | null }[]>('GET', '/mount/scan')
      setFound(d)
      if (d.length && !d.some((x) => x.address === bleAddr)) setBleAddr(d[0].address)
    } finally { setScanning(false) }
  })
  useEffect(() => { api<string[]>('GET', '/mount/drivers').then(setDrivers).catch((e) => reportError(e, 'Listing the mount drivers')) }, [])
  // Lists the serial ports at once, then asks each one (read-only) which mount is behind it, so nobody has to know a device path or a driver name.
  const scan = async () => {
    setProbing(true)
    try {
      const plain = await api<Port[]>('GET', '/mount/ports')
      setPorts(plain)
      const p = await api<Port[]>('GET', '/mount/detect')
      setPorts(p)
      const saved = p.find((x) => x.path === device)
      const known = saved?.driver ? saved : p.find((x) => x.driver)
      // adopt a recognised mount; otherwise keep a saved path that still exists, or take the first port
      if (known?.driver && (known.path !== device || driver !== known.driver)) { setDevice(known.path); setDriver(known.driver) }
      else if (!saved) setDevice(p[0]?.path ?? '')
    } catch (e) { reportError(e as Error, 'Looking for the mount') } finally { setProbing(false) }
  }
  useEffect(() => { void scan() }, []) // eslint-disable-line react-hooks/exhaustive-deps
  return (
    <Section title="Connect the mount" error={err}>
      <p className="lead">Connect the mount to slew, track and hold the target in view.</p>
      <div className="field">
        <label htmlFor="mount-driver">Mount</label>
        <select id="mount-driver" value={driver} onChange={(e) => setDriver(e.target.value)}>
          {(drivers.length ? drivers : ['zwo', 'simulator']).map((d) => <option key={d}>{d}</option>)}
        </select>
      </div>
      {driver === 'zwo' && (
        <div className="field">
          <label htmlFor="mount-transport">Connection</label>
          <select id="mount-transport" value={transport} onChange={(e) => setTransport(e.target.value as 'usb' | 'ble')}>
            <option value="usb">USB cable</option>
            <option value="ble">Bluetooth</option>
          </select>
        </div>
      )}
      {driver === 'zwo' && transport === 'ble' && (
        <div className="field">
          <div className="btnrow tight">
            <button type="button" className="grow" disabled={busy || scanning} onClick={scanBle}>{scanning ? 'Scanning (about 6 s)...' : 'Scan for mounts'}</button>
          </div>
          {found && found.length > 0 && (
            <ul className="mount-found" role="radiogroup" aria-label="Mounts found">
              {found.map((d) => (
                <li key={d.address}>
                  <button type="button" role="radio" aria-checked={bleAddr === d.address} className="mount-pick" onClick={() => setBleAddr(d.address)}>
                    <MountMark />
                    <span className="mp-name">{d.name}</span>
                    <SignalBars rssi={d.rssi} />
                    <span className="mp-state">{bleAddr === d.address ? 'Selected' : 'Select'}</span>
                  </button>
                </li>
              ))}
            </ul>
          )}
          {found && found.length === 0 && <div className="hint-text">No mount found.</div>}
          <div className="hint-text">Press the mount's pairing button, close the ZWO phone app (the mount talks to one device at a time) and keep the mount within a few metres.</div>
        </div>
      )}
      {driver === 'ioptron' && <div className="hint-text">iOptron mounts (CEM70 series and relatives, HAE69): connect the mount's USB port to the Pi. It appears as a serial port; the 115200 baud setting is applied for you. Set the mount's own time zone and site once on its hand controller, or let RustFlare send them with Go to Sun. Not yet tried on a real mount.</div>}
      {driver !== 'simulator' && !(driver === 'zwo' && transport === 'ble') && (
        <div className="field">
          <label id="mount-ports-label">USB mount</label>
          {ports.length > 0 && (
            <ul className="mount-found" role="radiogroup" aria-labelledby="mount-ports-label">
              {ports.map((p) => (
                <li key={p.path}>
                  <button type="button" role="radio" aria-checked={device === p.path} className="mount-pick" onClick={() => { setDevice(p.path); if (p.driver) setDriver(p.driver) }}>
                    <MountMark />
                    <span className="mp-name">{p.model ?? (probing ? 'looking…' : 'a serial device (no mount answered)')}<span className="mp-sub">{p.label.replace(/^usb-/, '').replace(/-if00.*$/, '')}</span></span>
                    <span className="mp-state">{device === p.path ? 'Selected' : 'Select'}</span>
                  </button>
                </li>
              ))}
            </ul>
          )}
          <div className="btnrow tight">
            <button type="button" className="grow" disabled={probing} onClick={() => void scan()}>{probing ? 'Looking for the mount…' : 'Find my mount'}</button>
          </div>
          {ports.length === 0 && !probing && <div className="hint-text">No serial devices found: is the mount's USB cable plugged in and the mount powered?</div>}
          {ports.length > 0 && !probing && !ports.some((p) => p.driver) && <div className="hint-text">The mount did not answer on any port. Switch it on, check the cable, and pick the driver above: iOptron mounts need their USB-to-serial adapter plugged into the mount's RS-232 or USB socket.</div>}
          <Disclosure title="Type a device path" summary={device || 'none'}>
            <input id="mount-device" aria-label="Serial device path" value={device} onChange={(e) => setDevice(e.target.value)} />
          </Disclosure>
        </div>
      )}
      <button type="button" className="primary wide" disabled={busy || scanning || (driver === 'zwo' && transport === 'ble' && !bleAddr)} onClick={() => run(() => api('POST', '/mount/connect', driver === 'zwo' && transport === 'ble' ? { driver, transport: 'ble', device: bleAddr } : { driver, device: driver === 'simulator' ? null : device }))}>{busy && transport === 'ble' && driver === 'zwo' ? 'Connecting over Bluetooth...' : 'Connect mount'}</button>
    </Section>
  )
}

/** How far the Sun is from the frame centre and which way to push (the original Centering panel, reduced to what you read at the eyepiece). */
function Framing({ status }: { status: Status }) {
  const cam = status.camera
  const sun = status.sun
  if (!cam || !sun?.partial || sun.radius <= Math.min(cam.width, cam.height) / 2) return null
  return (
    <Disclosure title="Framing a region of interest">
      <p className="hint-text">
        Only part of the disc is in view and its centre is {sun.pos.x < 0 || sun.pos.x > cam.width || sun.pos.y < 0 || sun.pos.y > cam.height ? 'outside' : 'near the edge of'} the frame. Slew the region you want into view, then go to Track and press <b>Hold position</b> (limb tracking keeps it there). "Centre and hold" moves the disc <i>centre</i> to the middle of the frame, which is only useful if you want that.
      </p>
    </Disclosure>
  )
}

function Offset({ status }: { status: Status }) {
  const cam = status.camera
  const sun = status.sun
  if (!cam) return null
  const dx = sun ? cam.width / 2 - sun.pos.x : 0 // image must move by (dx, dy) to centre the Sun
  const dy = sun ? cam.height / 2 - sun.pos.y : 0
  const dist = Math.hypot(dx, dy)
  const centered = !!sun && dist < Math.max(10, sun.radius * 0.03)
  let pad: string | null = null
  const cal = status.guiding.calibration
  if (cal && sun && !centered) {
    const p = cal.primary_dir, s = cal.secondary_dir
    const det = p.x * s.y - p.y * s.x
    if (Math.abs(det) > 1e-6) {
      const a = (s.y * dx - s.x * dy) / det
      const b = (-p.y * dx + p.x * dy) / det
      // the pad's letters are the mount's axes: which ones make the image go where the readout says
      pad = `${a >= 0 ? 'E' : 'W'} and ${b >= 0 ? 'N' : 'S'}`
    }
  }
  const arrow = (d: number, pos: string, neg: string) => (Math.abs(d) < 1 ? '' : d > 0 ? pos : neg)
  return (
    <div className="offset" aria-live="polite">
      {/* three fixed rows whatever the state, so the panel never changes size and pushes the pad around */}
      <div className={`offset-big ${centered ? 'ok' : ''} ${sun ? '' : 'muted'}`}>{!sun ? `No ${noun(profileOf(status))} found` : centered ? 'Centred' : `${dist.toFixed(0)} px off centre`}</div>
      <div className="offset-move">
        {!sun ? <span className="muted">Too dark? Try more exposure.</span> : centered ? <span className="muted">Nothing to move.</span> : (
          <>
            Move the image <b>{arrow(dx, '→', '←')}{Math.abs(dx) >= 1 ? ` ${Math.abs(dx).toFixed(0)} px` : ''}</b>{' '}
            <b>{arrow(dy, '↓', '↑')}{Math.abs(dy) >= 1 ? ` ${Math.abs(dy).toFixed(0)} px` : ''}</b>
            {pad ? <span className="muted"> · press {pad}</span> : <span className="muted"> · calibrate to see which button</span>}
          </>
        )}
      </div>
      <div className="hint-text offset-info">
        {sun ? <>{noun(profileOf(status))[0].toUpperCase() + noun(profileOf(status)).slice(1)} centre {sun.pos.x.toFixed(0)}, {sun.pos.y.toFixed(0)} · radius {sun.radius.toFixed(0)} px{sun.partial ? ' (partial, fitted)' : ''} · {((dist / sun.radius) * 100).toFixed(0)}% of radius</> : '\u00a0'}
      </div>
    </div>
  )
}

/** The hold-to-move slew pad: pointer down starts the slew, pointer up (or cancel, or leaving the tab) stops it. */
function Pad({ status, locked }: { status: Status; locked: boolean }) {
  const [saved, setSpeed] = useSetting('mount.slew_speed', 8)
  // the speeds come from the mount's driver (mounts differ); the saved choice snaps to the nearest one this mount has
  const speeds = status.mount?.slew_speeds?.length ? status.mount.slew_speeds : SPEEDS
  const speed = speeds.reduce((best, s) => (Math.abs(Math.log(s / saved)) < Math.abs(Math.log(best / saved)) ? s : best), speeds[0])
  const [err, setErr] = useState<string | null>(null)
  const [active, setActive] = useState<string | null>(null)
  const held = useRef(new Map<string, Axis>())
  const speedRef = useRef(speed)
  speedRef.current = speed
  const m = status.mount

  const send = (axis: Axis, s: number) => api('POST', '/mount/slew', { axis, speed: s }).then(() => setErr(null)).catch((e: Error) => setErr(e.message))
  const start = (key: string, axis: Axis, dir: 1 | -1) => {
    if (held.current.has(key)) return
    held.current.set(key, axis)
    setActive(key)
    send(axis, dir * speedRef.current)
  }
  const stop = (key: string) => {
    const axis = held.current.get(key)
    if (!axis) return
    held.current.delete(key)
    setActive((a) => (a === key ? null : a))
    send(axis, 0)
  }
  const stopAll = () => { for (const k of Array.from(held.current.keys())) stop(k) }

  useEffect(() => {
    const keys: Record<string, [Axis, 1 | -1]> = { ArrowUp: ['secondary', 1], ArrowDown: ['secondary', -1], ArrowLeft: ['primary', -1], ArrowRight: ['primary', 1] }
    const typing = (t: EventTarget | null) => t instanceof HTMLElement && (/^(INPUT|SELECT|TEXTAREA)$/.test(t.tagName) || !!t.closest('[role=tablist]'))
    const down = (e: KeyboardEvent) => { const k = keys[e.key]; if (!k || e.repeat || typing(e.target) || e.altKey || e.ctrlKey || e.metaKey) return; e.preventDefault(); start(e.key, k[0], k[1]) }
    const up = (e: KeyboardEvent) => { if (keys[e.key]) stop(e.key) }
    window.addEventListener('keydown', down)
    window.addEventListener('keyup', up)
    window.addEventListener('blur', stopAll)
    document.addEventListener('visibilitychange', stopAll)
    window.addEventListener('pagehide', stopAll)
    return () => {
      window.removeEventListener('keydown', down)
      window.removeEventListener('keyup', up)
      window.removeEventListener('blur', stopAll)
      document.removeEventListener('visibilitychange', stopAll)
      window.removeEventListener('pagehide', stopAll)
      stopAll() // never leave the mount running when this tab is left
    }
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  const btn = (key: string, label: string, name: string, axis: Axis, dir: 1 | -1) => (
    <button
      type="button"
      className={`pad-btn pad-${name[0].toLowerCase()} ${active === key ? 'held' : ''}`}
      disabled={locked}
      aria-label={`${name}: hold to move`}
      aria-pressed={active === key}
      onPointerDown={(e) => { e.preventDefault(); try { (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId) } catch { /* synthetic pointer */ } start(key, axis, dir) }}
      onPointerUp={() => stop(key)}
      onPointerCancel={() => stop(key)}
      onLostPointerCapture={() => stop(key)}
      onContextMenu={(e) => e.preventDefault()}
      onKeyDown={(e) => { if ((e.key === ' ' || e.key === 'Enter') && !e.repeat) { e.preventDefault(); start(key, axis, dir) } }}
      onKeyUp={(e) => { if (e.key === ' ' || e.key === 'Enter') stop(key) }}
      onBlur={() => stop(key)}
    >{label}</button>
  )

  return (
    <div>
      <div className="pad" role="group" aria-label="Slew pad. Hold a direction to move, release to stop.">
        <span className="pad-gap" />{btn('ArrowUp', 'N', 'North', 'secondary', 1)}<span className="pad-gap" />
        {btn('ArrowLeft', 'W', 'West', 'primary', -1)}
        <button type="button" className="pad-stop" onClick={() => { stopAll(); api('POST', '/mount/stop').catch((e: Error) => setErr(e.message)) }}>STOP</button>
        {btn('ArrowRight', 'E', 'East', 'primary', 1)}
        <span className="pad-gap" />{btn('ArrowDown', 'S', 'South', 'secondary', -1)}<span className="pad-gap" />
      </div>
      <div className="pad-foot">
        <label htmlFor="slew-speed">Slew speed</label>
        <select id="slew-speed" value={speed} onChange={(e) => setSpeed(parseFloat(e.target.value))} disabled={!m}>
          {speeds.map((s) => <option key={s} value={s}>{s}× sidereal</option>)}
        </select>
      </div>
      <div className="hint-text">Hold a direction to move, release to stop.</div>
      <ErrorLine error={err} />
    </div>
  )
}

/** A plain equatorial mount, drawn: tripod stub, head, counterweight bar and the telescope saddle. */
function MountMark() {
  return (
    <svg className="mp-icon" viewBox="0 0 32 32" width="28" height="28" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      <path d="M11 28h10M13 28l1.5-6h3L19 28" /><path d="M14.5 22l-2.5-8 8-3 3 8z" /><path d="M9 9l14-4M11 5.5l12-3.5" /><circle cx="22" cy="12" r="1.6" />
    </svg>
  )
}

/** Signal strength as four bars; the dBm is in the label for screen readers, the picture is for people. */
function SignalBars({ rssi }: { rssi: number | null }) {
  const n = rssi == null ? 0 : rssi >= -60 ? 4 : rssi >= -70 ? 3 : rssi >= -80 ? 2 : 1
  return (
    <span className="mp-bars" role="img" aria-label={rssi == null ? 'signal unknown' : `signal ${rssi} dBm, ${n} of 4`}>
      {[1, 2, 3, 4].map((i) => <i key={i} className={i <= n ? 'on' : ''} style={{ height: `${i * 4 + 2}px` }} />)}
    </span>
  )
}

type MountSt = NonNullable<Status['mount']>

/** "ZWO AM3 on /dev/serial/by-id/usb-ZWO_…" -> "ZWO AM3": the device path is noise on a phone. */
const shortInfo = (s: string) => s.replace(/ on \/dev\/\S+/, '')

/** What the mount itself reports. The tracking switch above follows the same field, so pressing park or home on the mount flips it. */
function MountState({ m }: { m: MountSt }) {
  const tags: { t: string; tone?: string }[] = [
    ...(m.link ? [{ t: m.link === 'ble' ? 'Bluetooth' : 'USB', tone: m.link_up === false ? 'warn' : undefined }] : []),
    ...(m.link_up === false ? [{ t: 'Link lost', tone: 'warn' }] : []),
    { t: m.tracking ? 'Tracking' : 'Stopped', tone: m.tracking ? 'ok' : 'warn' },
    ...(m.slewing ? [{ t: 'Slewing', tone: 'info' }] : []),
    ...(m.at_home ? [{ t: 'At home' }] : []),
    ...(m.park === 'parked' ? [{ t: 'Parked', tone: 'warn' }] : m.park === 'parking' ? [{ t: 'Parking', tone: 'warn' }] : []),
    ...(m.alt_az ? [{ t: 'Alt-az' }] : []),
  ]
  return (
    <ul className="tags" aria-label="Mount state">
      {tags.map((x) => <li key={x.t} className={`tag ${x.tone ?? ''}`}>{x.t}</li>)}
    </ul>
  )
}

/** Send the mount to its home position, after an explicit confirmation. There is deliberately no park or unpark here. */
function SendHome({ status, disabled, reason }: { status: Status; disabled: boolean; reason: string | null }) {
  const [ask, setAsk] = useState(false)
  const { err, busy, run } = useAction()
  if (!status.mount) return null
  return (
    <div className="home">
      {!ask ? (
        <button type="button" className="wide" disabled={disabled} aria-describedby={disabled && reason ? 'why-home' : undefined} onClick={() => setAsk(true)}>Send home</button>
      ) : (
        <div className="confirm" role="alertdialog" aria-label="Send the mount home">
          <p>The mount will slew to its home position. Make sure the telescope and cables are clear.</p>
          <div className="btnrow tight">
            <button type="button" className="grow" onClick={() => setAsk(false)}>Cancel</button>
            <button type="button" className="danger grow" disabled={busy || disabled} onClick={() => run(async () => { await api('POST', '/mount/home', { confirm: true }); setAsk(false) })}>Send home</button>
          </div>
        </div>
      )}
      {disabled && reason && <div className="gate-why hint-text" id="why-home">{reason}</div>}
      <ErrorLine error={err} />
      {status.mount.can_search_home && <SearchHome disabled={disabled} />}
    </div>
  )
}

/** Not the same as Send home: this finds the mount's mechanical zero with its homing sensors and makes it the new zero position. */
function SearchHome({ disabled }: { disabled: boolean }) {
  const [ask, setAsk] = useState(false)
  const { err, busy, run } = useAction()
  return (
    <div className="home">
      {!ask ? (
        <button type="button" className="wide" disabled={disabled} onClick={() => setAsk(true)}>Search home…</button>
      ) : (
        <div className="confirm" role="alertdialog" aria-label="Search for the home position">
          <p>The mount will move both axes until its home sensors find the mechanical zero position, and <b>make that its new zero</b>. Send home only goes to the zero it already has. Use this when the zero is wrong; make sure the telescope and cables are clear, with someone at the mount.</p>
          <div className="btnrow tight">
            <button type="button" className="grow" onClick={() => setAsk(false)}>Cancel</button>
            <button type="button" className="danger grow" disabled={busy || disabled} onClick={() => run(async () => { await api('POST', '/mount/search-home', { confirm: true }); setAsk(false) })}>Search home</button>
          </div>
        </div>
      )}
      <ErrorLine error={err} />
    </div>
  )
}

/** One tap saves where this phone is as the site (the browser only shares its location over HTTPS; the failure is said in the block that asked). */
function saveMyLocation(run: (f: () => Promise<unknown>) => unknown) {
  void run(async () => { const p = await locate(); await api('PUT', '/site', p) })
}

/** What the mount itself holds as its site and time, next to the Pi's, and a button that sends the Pi's (nothing moves). */
function MountClock({ status, openMore, focus }: { status: Status; openMore?: (focus: string) => void; focus?: { id: string; nonce: number } | null }) {
  const { err, busy, run } = useAction()
  const [open, setOpen] = useState(focus?.id === 'clock')
  useEffect(() => { if (focus?.id === 'clock') setOpen(true) }, [focus?.nonce]) // eslint-disable-line react-hooks/exhaustive-deps
  const m = status.mount!
  const st = m.site_time
  const ntp = status.clock?.ntp_synchronized
  const reason = sendReason(status)
  const zone = st?.pi.utc_offset_minutes ?? null
  const row = (label: string, mine: string, pi: string) => (
    <tr><th scope="row">{label}</th><td>{mine}</td><td>{pi}</td></tr>
  )
  return (
    <Disclosure title="Mount time and place" open={open} onToggle={setOpen} summary={<span className="mc-sum">{clockSummary(st)}</span>}>
      <div className="mountclock">
        {!st ? (
          <p className="hint-text">This mount cannot report its site and time, so RustFlare cannot compare them with the Pi's.</p>
        ) : (
          <>
            <table className="clocktable">
              <thead><tr><td /><th scope="col">Mount</th><th scope="col">Pi</th></tr></thead>
              <tbody>
                {row('Time', timeText(st.mount, zone), timeText(st.pi, zone))}
                {row('Place', placeText(st.mount), placeText(st.pi))}
              </tbody>
            </table>
            <p className="hint-text" role="status">{st.why}</p>
          </>
        )}
        <p className="hint-text">The Pi's clock: {ntp === true ? 'synchronised to a time server' : ntp === false ? 'NOT synchronised to a time server: fix the Pi first, or what it sends the mount is wrong too' : 'synchronisation unknown'}. Go to sends the Pi's site and time to the mount first, so this matters most if you also steer from the mount's own handset.</p>
        <button type="button" className="wide" disabled={busy || !!reason} aria-describedby={reason ? 'why-sendtime' : undefined} onClick={() => run(() => api('POST', '/mount/sync-site-time'))}>{busy ? 'Sending…' : 'Send site and time to the mount'}</button>
        {reason && <div className="gate-why hint-text" id="why-sendtime">{reason}</div>}
        <Switch label="Send them by themselves when connecting" checked={!!status.mount?.auto_sync_site_time} onChange={(on) => run(() => api('PUT', '/mount/auto-site-time', { enabled: on }))}
          hint="For mounts without a clock battery. After connecting, the Pi sends its site and time if they differ, when its own clock is synchronised, a site is saved and the mount is not moving. It moves nothing." />
        {st && !status.site && (
          <div className="needs" role="note">
            <b>Set your site first.</b> The Pi sends its saved position and clock.
            <SiteChoices busy={busy} onUse={() => saveMyLocation(run)} onType={openMore && (() => openMore('site'))} />
          </div>
        )}
        <ErrorLine error={err} />
      </div>
    </Disclosure>
  )
}

/** Slew to where the Sun, Moon or a bright planet is now, then switch tracking on at its rate. Needs a saved site and a trustworthy clock. */
function GoToSun({ status, disabled, openMore }: { status: Status; disabled: boolean; openMore?: (focus: string) => void }) {
  const [ask, setAsk] = useState(false)
  const [target, setTarget] = useState('sun') // not remembered: a button that moves the mount should always start at the Sun
  const { err, busy, run } = useAction()
  const clockBad = status.clock?.ntp_synchronized === false
  const bodies = status.sky_targets ?? []
  const planet = bodies.find((b) => b.id === target)
  const sky = status.sky_sun
  const name = planet?.name ?? 'the Sun'
  const alt = planet ? planet.altitude_deg : sky?.altitude_deg
  const prof = profileOf(status)
  const wantProfile = profileForTarget(target)
  const offerSwitch = wantProfile ? (prof !== wantProfile ? wantProfile : null) : (isSun(prof) ? null : 'sun_ha')
  const below = alt != null && alt < -1
  const st = status.mount?.site_time
  const mountClockDiffers = st?.time_ok === false && st?.place_ok === false ? 'clock and place' : st?.time_ok === false ? `clock (${st.time_diff_s != null ? spanWords(st.time_diff_s) : 'off'})` : st?.place_ok === false ? 'place' : ''
  const low = alt != null && alt >= -1 && alt < 15
  // the site is a once-only setting, so the fix is here: one tap saves where this phone is
  const useHere = () => saveMyLocation(run)
  const go = () => run(async () => { await api('POST', '/mount/goto', { target: planet ? planet.id : 'sun', confirm: true, allow_unsynced_clock: clockBad, allow_below_horizon: false }); setAsk(false) })
  if (status.goto) return <div className="hint-text" role="status">Going to {bodies.find((b) => b.id === status.goto?.target)?.name ?? 'the Sun'} ({status.goto.phase}). STOP cancels.</div>
  const label = (b: { name: string; altitude_deg: number }) => `${b.name} · ${b.altitude_deg < -1 ? 'not up (below the horizon)' : `${b.altitude_deg.toFixed(0)}° up`}`
  return (
    <div className="home">
      {!ask ? (
        <>
          {bodies.length > 0 && (
            <select className="goto-target" aria-label="Go to target" value={planet ? planet.id : 'sun'} onChange={(e) => setTarget(e.target.value)}>
              <option value="sun">{sky ? label({ name: 'Sun', altitude_deg: sky.altitude_deg }) : 'Sun'}</option>
              {bodies.map((b) => <option key={b.id} value={b.id}>{label(b)}</option>)}
            </select>
          )}
          <button type="button" className="wide primary" disabled={disabled || !status.site || below} aria-describedby={!status.site ? 'why-goto-site' : below ? 'why-goto-below' : undefined} title={below ? `${planet ? planet.name : 'The Sun'} is below the horizon` : undefined} onClick={() => setAsk(true)}>Go to {planet ? planet.name : 'Sun'}</button>
          {!status.site && (
            <div className="needs" role="note" id="why-goto-site">
              <b>Set your site first.</b> Go to works out where the Sun is from your position and the clock.
              <SiteChoices busy={busy} onUse={useHere} onType={openMore && (() => openMore('site'))} />
            </div>
          )}
        </>
      ) : (
        <div className="confirm" role="alertdialog" aria-label={`Go to ${name}`}>
          <p>Go to sends the Pi's site and time to the mount first, slews to where {name} is now, then starts {planet ? (planet.id === 'moon' ? 'lunar' : 'sidereal') : 'solar'} tracking. Make sure the telescope and cables are clear.{planet ? ' The position is good to a few arcminutes: centre it by hand.' : ''}</p>
          {mountClockDiffers && <p className="warn">The mount's {mountClockDiffers} differs from the Pi's; the Pi's values are sent first.</p>}
          {low && <p className="warn">{name} is only {alt!.toFixed(0)}° up: expect trees, buildings and poor seeing.</p>}
          {clockBad && <p className="warn">The Pi's clock is not synchronised, so the position may be off.</p>}
          <div className="btnrow tight">
            <button type="button" className="grow" onClick={() => setAsk(false)}>Cancel</button>
            <button type="button" className="danger grow" disabled={busy} onClick={go}>{`Go to ${planet ? planet.name : 'Sun'}`}</button>
          </div>
        </div>
      )}
      {offerSwitch && (
        <div className="needs" role="note">
          You are on the <b>{PROFILES.find((x) => x.id === prof)?.label}</b> profile and chose {name}. Switch to <b>{PROFILES.find((x) => x.id === offerSwitch)?.label}</b> so the words and the tools match?
          <div className="btnrow tight"><button type="button" className="grow" onClick={() => run(() => api('PUT', '/profile', { id: offerSwitch }))}>Switch to {PROFILES.find((x) => x.id === offerSwitch)?.label}</button></div>
        </div>
      )}
      {alt != null && !planet && sky && <div className={below ? 'gate-why hint-text' : 'hint-text'} id={below ? 'why-goto-below' : undefined}>Sun now: altitude {sky.altitude_deg.toFixed(1)}°, azimuth {sky.azimuth_deg.toFixed(0)}°{sky.altitude_deg < 0 ? ' (below the horizon: Go to is off until it rises)' : ''}</div>}
      {planet && <div className={below ? 'gate-why hint-text' : 'hint-text'} id={below ? 'why-goto-below' : undefined}>{planet.name} now: altitude {planet.altitude_deg.toFixed(1)}°, azimuth {planet.azimuth_deg.toFixed(0)}°{below ? ' (below the horizon: Go to is off until it rises)' : ''}</div>}
      <ErrorLine error={err} />
    </div>
  )
}

import { MeridianFlip } from './MeridianFlip'

const RATE_NAME = { solar: 'Solar', lunar: 'Lunar', sidereal: 'Star' } as const
const RATE_FOR = { solar: 'Solar (Sun)', lunar: 'Lunar (Moon)', sidereal: 'Star (planets)' } as const

/** Step 2: connect the mount, switch on solar tracking, slew the region of interest into view. */
export function AimTab({ status, next, openMore, focus }: { status: Status | null; next: () => void; openMore?: (focus: string) => void; focus?: { id: string; nonce: number } | null }) {
  const { err, run } = useAction()
  const m = status?.mount
  const sheet = useDatasheet('mount', status, m ? shortInfo(m.info) : null)
  if (!status || !m) return <MountConnect />
  const parked = m.park === 'parked' || m.park === 'parking'
  const wantRate = PROFILES.find((x) => x.id === profileOf(status))?.rate ?? null
  return (
    <>
      {parked && <div className="warn" role="alert">{m.park === 'parking' ? 'The mount is parking: motion is disabled.' : 'Parked on the mount: motion is disabled.'}</div>}
      <Offset status={status} />
      <Pad status={status} locked={parked} />
      <GoToSun status={status} disabled={parked || !!status.goto} openMore={openMore} />
      <MountClock status={status} openMore={openMore} focus={focus} />
      <Disclosure title="Meridian flip">
        <MeridianFlip status={status} />
      </Disclosure>
      <Section error={err}>
        <Switch label={`${RATE_NAME[m.rate ?? 'solar']} tracking`} checked={m.tracking} disabled={parked} onChange={(on) => run(() => api('POST', '/mount/tracking', { enabled: on }))} hint={m.tracking ? shortInfo(m.info) : `${shortInfo(m.info)}. Off: the target will drift out of view.`} />
        <div className="seg" role="group" aria-label="Tracking rate" aria-describedby="rate-note">
          {(['solar', 'lunar', 'sidereal'] as const).map((r) => (
            <button key={r} type="button" className={(m.rate ?? 'solar') === r ? 'on' : ''} aria-pressed={(m.rate ?? 'solar') === r} disabled={parked} onClick={() => run(() => api('POST', '/mount/tracking', { enabled: true, rate: r }))}>{RATE_NAME[r]}</button>
          ))}
        </div>
        <p className="hint-text" id="rate-note">{m.tracking ? 'Choosing a rate sets how fast the mount tracks.' : 'Tracking is off. Choosing a rate switches tracking on at that rate.'}</p>
        {wantRate && wantRate !== (m.rate ?? 'solar') && !parked && (
          <div className="needs" role="note">
            Tracking is {RATE_NAME[m.rate ?? 'solar']}. Switch to {RATE_FOR[wantRate]}?
            <div className="btnrow tight"><button type="button" className="grow" onClick={() => run(() => api('POST', '/mount/tracking', { enabled: true, rate: wantRate }))}>{m.tracking ? `Switch to ${RATE_NAME[wantRate]}` : `Switch to ${RATE_NAME[wantRate]} and track`}</button></div>
          </div>
        )}
        <MountState m={m} />
      </Section>
      <SendHome status={status} disabled={parked || status.capture.state !== 'idle'} reason={parked ? 'The mount is parked.' : status.capture.state !== 'idle' ? 'Not while recording.' : null} />
      <Framing status={status} />
      <DeviceCard kind="mount" data={sheet} />
      <div className="btnrow end">
        <button type="button" className="quiet" onClick={() => run(() => api('POST', '/mount/disconnect'))}>Disconnect mount</button>
        <button type="button" className="primary" onClick={next}>Next: Track →</button>
      </div>
    </>
  )
}
