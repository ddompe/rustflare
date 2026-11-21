import { useState } from 'react'
import { api, Status } from '../api'
import { Panel, Row, useAction } from './ui'
import { useSetting } from '../settings'

const gb = (b: number) => (b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : `${(b / 1e6).toFixed(0)} MB`)

export function CapturePanel({ status }: { status: Status | null }) {
  const [name, setName] = useSetting('capture.name', '')
  const [mode, setMode] = useSetting<'sequence' | 'bursts'>('capture.mode', 'sequence')
  const [format, setFormat] = useSetting<'ser' | 'png'>('capture.format', 'ser')
  const [frames, setFrames] = useSetting('capture.frames', '1000')
  const [seconds, setSeconds] = useSetting('capture.seconds', '')
  const [every, setEvery] = useSetting('capture.every', '30')
  const [bursts, setBursts] = useSetting('capture.bursts', '')
  const [flatFrames, setFlatFrames] = useSetting('capture.flat_frames', '50')
  const [level, setLevel] = useSetting('capture.flat_level', '50')
  const [bufMb, setBufMb] = useState('')
  const { err, run } = useAction()
  const c = status?.capture
  const cam = status?.camera
  const busy = !!c && c.state !== 'idle'
  const active = !!c && ['single', 'sequence', 'bursts', 'flats'].includes(c.state)
  const n = (s: string) => (s.trim() === '' ? undefined : parseFloat(s))
  const nm = name.trim() || undefined

  if (!cam) return <Panel title="Capture"><span className="muted">Open a camera first</span></Panel>

  // planning aid: how big is a burst, does it fit in the RAM buffer, is the disk likely to keep up?
  const frameBytes = cam.width * cam.height * (cam.bits / 8)
  const nFrames = n(frames) ?? (n(seconds) && cam.fps ? n(seconds)! * cam.fps : 0)
  const burstBytes = nFrames * frameBytes
  const bufFrames = frameBytes ? Math.floor(((c?.buffer_mb ?? 1024) * 1048576) / frameBytes) : 0
  const burstSecs = cam.fps ? nFrames / cam.fps : 0
  const every_s = n(every) ?? 0
  const diskNeed = burstSecs && every_s ? burstBytes / Math.max(every_s, burstSecs) / 1e6 : 0 // average MB/s needed

  const start = () =>
    mode === 'sequence'
      ? run(() => api('POST', '/capture/sequence', { name: nm, frames: n(frames), seconds: n(seconds), format }))
      : run(() => api('POST', '/capture/bursts', { name: nm, frames: n(frames), seconds: n(seconds), interval_s: n(every), bursts: n(bursts), format }))

  return (
    <Panel title="Capture" error={err}>
      <Row label="Name"><input value={name} onChange={(e) => setName(e.target.value)} placeholder="optional, e.g. AR3842" /></Row>
      <Row><button disabled={busy} onClick={() => run(() => api('POST', '/capture/single', { name: nm }))}>Single frame (PNG)</button></Row>
      <hr />
      <Row label="Record">
        <select value={mode} onChange={(e) => setMode(e.target.value as 'sequence' | 'bursts')}>
          <option value="sequence">One sequence</option>
          <option value="bursts">Bursts every N seconds</option>
        </select>
        <select value={format} onChange={(e) => setFormat(e.target.value as 'ser' | 'png')} title="File format">
          <option value="ser">SER video</option>
          <option value="png">PNG images</option>
        </select>
      </Row>
      <Row label={mode === 'bursts' ? 'Frames / burst' : 'Frames'}><input value={frames} onChange={(e) => setFrames(e.target.value)} /></Row>
      <Row label="or seconds"><input value={seconds} onChange={(e) => setSeconds(e.target.value)} placeholder="stop after…" /></Row>
      {mode === 'bursts' && (
        <>
          <Row label="Every (s)"><input value={every} onChange={(e) => setEvery(e.target.value)} placeholder="start to start" /></Row>
          <Row label="Bursts"><input value={bursts} onChange={(e) => setBursts(e.target.value)} placeholder="blank = until stopped" /></Row>
        </>
      )}
      {nFrames > 0 && (
        <div className="muted small">
          {mode === 'bursts' ? 'Each burst' : 'This recording'}: ≈ {Math.round(nFrames)} frames, {gb(burstBytes)}{burstSecs ? `, ≈ ${burstSecs.toFixed(1)} s at ${cam.fps.toFixed(0)} fps` : ''}.
          {bufFrames > 0 && (nFrames <= bufFrames
            ? ` Fits in the RAM buffer (${bufFrames} frames), so the disk can be slower than the camera.`
            : ` Larger than the RAM buffer (${bufFrames} frames): once it is full, capture slows to disk speed.`)}
          {mode === 'bursts' && diskNeed > 0 && ` Average disk load ≈ ${diskNeed.toFixed(0)} MB/s.`}
        </div>
      )}
      <Row><button className="primary" disabled={busy} onClick={start}>{mode === 'bursts' ? 'Start bursts' : 'Record sequence'}</button></Row>
      <hr />
      <div className="muted small"><b>Flats</b> (preliminary: a guided "cap on, adjust, OK" workflow is planned)</div>
      <Row label="Flat frames"><input value={flatFrames} onChange={(e) => setFlatFrames(e.target.value)} /></Row>
      <Row label="Target level %"><input value={level} onChange={(e) => setLevel(e.target.value)} /></Row>
      <Row><button disabled={busy} onClick={() => run(() => api('POST', '/capture/flats', { name: nm, frames: n(flatFrames), level: (n(level) ?? 50) / 100 }))}>Capture flats</button></Row>
      <div className="muted small">Starts immediately: put the flat cap or diffuser on first. Exposure is adjusted automatically and restored afterwards; tracking is stopped.</div>

      {c && c.state !== 'idle' && (
        <div className="progress">
          <div><b>{c.state}</b>: {c.phase}</div>
          {c.burst != null && <div className="muted small">burst {c.burst}{c.bursts_total ? ` of ${c.bursts_total}` : ''}{c.next_in_s != null ? ` · next in ${c.next_in_s.toFixed(0)} s` : ''}</div>}
          {c.total != null && c.state !== 'flushing' && <progress max={c.total} value={c.done} />}
          {c.state !== 'flats' && c.buffer_capacity > 0 && (
            <>
              <progress max={c.buffer_capacity} value={c.buffer_queued} className="buf" />
              <div className="muted small">RAM buffer {c.buffer_queued}/{c.buffer_capacity} frames · disk writing {c.disk_mb_s.toFixed(0)} MB/s</div>
            </>
          )}
          <div className="muted small">{c.done}{c.total != null ? ` / ${c.total}` : ''} frames {c.file && `· ${c.file}`}</div>
          {active && <button className="danger" onClick={() => run(() => api('POST', '/capture/stop'))}>Stop</button>}
        </div>
      )}
      {c && c.state === 'idle' && c.last_file && <div className="muted small">Last: <a href={`/files/${c.last_file}`}>{c.last_file}</a> ({c.phase}{c.disk_mb_s > 0 ? `, written at ${c.disk_mb_s.toFixed(0)} MB/s` : ''})</div>}

      <hr />
      <Row label="RAM buffer">
        <input className="num" value={bufMb || String(c?.buffer_mb ?? 1024)} onChange={(e) => setBufMb(e.target.value)} />
        <span className="unit">MB</span>
        <button disabled={busy || !bufMb} onClick={() => run(async () => { await api('PUT', '/capture/buffer', { mb: parseFloat(bufMb) }); setBufMb('') })}>Set</button>
      </Row>
      <div className="muted small">
        Frames are captured into memory and written to disk in the background. {c?.ram_available_mb ? `${c.ram_available_mb} MB of RAM free; the buffer is capped to 70% of that. ` : ''}
        Holds ≈ {bufFrames} frames at the current size.
      </div>
    </Panel>
  )
}
