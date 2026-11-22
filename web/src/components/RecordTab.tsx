import { api, Status } from '../api'
import { Disclosure, Empty, Field, Section, Segmented, useAction } from './ui'
import { StorageLine } from './Storage'
import { FileManagerButton } from './FileManager'
import { ProminencePair } from './ProminencePair'
import { BracketLanes } from './BracketLanes'
import { useSetting } from '../settings'

const gb = (b: number) => (b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : `${(b / 1e6).toFixed(0)} MB`)

/** Live progress of whatever is being captured; also used in the status strip's Stop button. */
export function Progress({ status, onStop }: { status: Status; onStop: () => void }) {
  const c = status.capture
  const active = ['single', 'sequence', 'bursts', 'flats'].includes(c.state)
  return (
    <div className="progress">
      <div className="prog-title"><b>{c.state}</b>: {c.phase}</div>
      {c.burst != null && <div className="muted">burst {c.burst}{c.bursts_total ? ` of ${c.bursts_total}` : ''}{c.next_in_s != null ? ` · next in ${c.next_in_s.toFixed(0)} s` : ''}</div>}
      {c.total != null && c.state !== 'flushing' && <progress max={c.total} value={c.done} aria-label="Frames captured" />}
      {c.state !== 'flats' && c.buffer_capacity > 0 && (
        <>
          <progress max={c.buffer_capacity} value={c.buffer_queued} className="buf" aria-label="RAM buffer in use" />
          <div className="hint-text">RAM buffer {c.buffer_queued}/{c.buffer_capacity} frames · disk writing {c.disk_mb_s.toFixed(0)} MB/s</div>
        </>
      )}
      <div className="hint-text">{c.done}{c.total != null ? ` / ${c.total}` : ''} frames {c.file && `· ${c.file}`}</div>
      {active && <button type="button" className="danger wide big" onClick={onStop}>Stop recording</button>}
    </div>
  )
}

/** Step 4: record. Defaults to the usual job (a burst every N seconds); everything rarer is in More. */
export function RecordTab({ status, goto, openFiles }: { status: Status | null; goto: (t: 'more', focus: string) => void; openFiles: () => void }) {
  const [name, setName] = useSetting('capture.name', '')
  const [mode, setMode] = useSetting<'sequence' | 'bursts'>('capture.mode', 'bursts')
  const [format, setFormat] = useSetting<'ser' | 'png' | 'fits'>('capture.format', 'ser')
  const [onFull, setOnFull] = useSetting<'wait' | 'drop'>('capture.on_full', 'wait')
  const [frames, setFrames] = useSetting('capture.frames', '100')
  const [seconds, setSeconds] = useSetting('capture.seconds', '')
  const [every, setEvery] = useSetting('capture.every', '30')
  const [bursts, setBursts] = useSetting('capture.bursts', '')
  const { err, run } = useAction()
  const c = status?.capture
  const cam = status?.camera
  const busy = !!c && c.state !== 'idle'
  const n = (s: string) => (s.trim() === '' ? undefined : parseFloat(s))
  const nm = name.trim() || undefined

  if (!cam || !status || !c) return <Empty>Open the camera first, then record here.</Empty>

  if (busy) {
    return (
      <Section title="Recording" error={err}>
        <Progress status={status} onStop={() => run(() => api('POST', '/capture/stop'))} />
        <StorageLine status={status} onChange={() => goto('more', 'storage')} />
      </Section>
    )
  }

  // planning aid: how big is a burst, does it fit in the RAM buffer, is the disk likely to keep up?
  const frameBytes = cam.width * cam.height * (cam.bits / 8)
  const nFrames = n(frames) ?? (n(seconds) && cam.fps ? n(seconds)! * cam.fps : 0)
  const burstBytes = nFrames * frameBytes
  const bufFrames = frameBytes ? Math.floor(((c.buffer_mb ?? 1024) * 1048576) / frameBytes) : 0
  const burstSecs = cam.fps ? nFrames / cam.fps : 0
  const every_s = n(every) ?? 0
  const diskNeed = burstSecs && every_s ? burstBytes / Math.max(every_s, burstSecs) / 1e6 : 0 // average MB/s needed

  const start = () =>
    mode === 'sequence'
      ? run(() => api('POST', '/capture/sequence', { name: nm, frames: n(frames), seconds: n(seconds), format, on_full: onFull }))
      : run(() => api('POST', '/capture/bursts', { name: nm, frames: n(frames), seconds: n(seconds), interval_s: n(every), bursts: n(bursts), format, on_full: onFull }))

  return (
    <>
      <Section title="Record" error={err}>
        <Segmented label="Recording mode" value={mode} onChange={setMode} options={[{ id: 'bursts', label: 'Bursts every N s' }, { id: 'sequence', label: 'One sequence' }]} />
        <div className="grid2">
          <Field label={mode === 'bursts' ? 'Frames per burst' : 'Frames'}>{(id) => <input id={id} inputMode="numeric" value={frames} onChange={(e) => setFrames(e.target.value)} />}</Field>
          <Field label="or seconds">{(id) => <input id={id} inputMode="decimal" value={seconds} onChange={(e) => setSeconds(e.target.value)} placeholder="stop after…" />}</Field>
          {mode === 'bursts' && (
            <>
              <Field label="Every (s)">{(id) => <input id={id} inputMode="decimal" value={every} onChange={(e) => setEvery(e.target.value)} placeholder="start to start" />}</Field>
              <Field label="Number of bursts">{(id) => <input id={id} inputMode="numeric" value={bursts} onChange={(e) => setBursts(e.target.value)} placeholder="until stopped" />}</Field>
            </>
          )}
        </div>
        {nFrames > 0 && (
          <p className="hint-text plan">
            {mode === 'bursts' ? 'Each burst' : 'This recording'}: ≈ {Math.round(nFrames)} frames, {gb(burstBytes)}{burstSecs ? `, ≈ ${burstSecs.toFixed(1)} s at ${cam.fps.toFixed(0)} fps` : ''}.
            {bufFrames > 0 && (nFrames <= bufFrames
              ? ` Fits in the RAM buffer (${bufFrames} frames), so the disk can be slower than the camera.`
              : ` Larger than the RAM buffer (${bufFrames} frames): once it is full, capture slows to disk speed.`)}
            {mode === 'bursts' && diskNeed > 0 && ` Average disk load ≈ ${diskNeed.toFixed(0)} MB/s.`}
          </p>
        )}
        <Segmented label="File format" value={format} onChange={setFormat} options={[{ id: 'ser', label: 'SER video' }, { id: 'png', label: 'PNG' }, { id: 'fits', label: 'FITS' }]} />
        <Segmented label="When the RAM buffer is full" value={onFull} onChange={setOnFull} options={[{ id: 'wait', label: 'Wait for the disk' }, { id: 'drop', label: 'Drop frames' }]} />
        <div className="hint-text">{onFull === 'drop' ? 'Keeps the camera at full speed; frames that do not fit are dropped and counted. Dropped frames never count toward the frame goal.' : 'Never loses a frame: a camera faster than the disk slows to disk speed once the buffer is full.'}{format === 'fits' ? ' FITS makes one file per frame (a folder per burst), with exposure, gain and time in the header.' : ''}</div>
        <StorageLine status={status} onChange={() => goto('more', 'storage')} />
        <div className="sticky-act"><button type="button" className="primary wide big" onClick={start}>{mode === 'bursts' ? 'Start bursts' : 'Start recording'}</button></div>
      </Section>
      <Disclosure title="Name and single frame">
        <Field label="Name (optional)">{(id) => <input id={id} value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. AR3842" />}</Field>
        <button type="button" className="wide" onClick={() => run(() => api('POST', '/capture/single', { name: nm }))}>Save a single frame (PNG)</button>
      </Disclosure>
      {c.last_file && <div className="hint-text lastfile">Last: <a href={`/files/${c.last_file}`}>{c.last_file}</a> ({c.phase}{c.disk_mb_s > 0 ? `, written at ${c.disk_mb_s.toFixed(0)} MB/s` : ''})</div>}
      <Disclosure title="Prominence pair" summary="disc + long exposure">
        <ProminencePair status={status} />
      </Disclosure>
      <Disclosure title="HDR bracketing lanes" summary="eclipse, Moon">
        <BracketLanes status={status} />
      </Disclosure>
      <FileManagerButton status={status} onOpen={openFiles} />
    </>
  )
}
