import { api, Status } from '../api'
import { Disclosure, Empty, endsText, Field, Section, Segmented, Switch, useAction } from './ui'
import { StorageLine } from './Storage'
import { FileManagerButton } from './FileManager'
import { ProminencePair } from './ProminencePair'
import { LaneEditor, parseLanes, PRESETS } from './BracketLanes'
import { useSetting } from '../settings'
import { recordRequest } from '../recipe'

const gb = (b: number) => (b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : `${(b / 1e6).toFixed(0)} MB`)

/** Live progress of whatever is being captured; also used in the status strip's Stop button. */
export function Progress({ status, onStop }: { status: Status; onStop: () => void }) {
  const c = status.capture
  const active = ['single', 'sequence', 'bursts', 'flats'].includes(c.state)
  return (
    <div className="progress">
      <div className="prog-title"><b>{c.state}</b>: {c.phase}</div>
      {c.burst != null && <div className="muted">burst {c.burst}{c.bursts_total ? ` of ${c.bursts_total}` : ''}{c.next_in_s != null ? ` · next in ${c.next_in_s.toFixed(0)} s` : ''}</div>}
      {c.eta_s != null && c.eta_s > 0 && <div className="muted">Ends {endsText(c.eta_s)}</div>}
      {c.total != null && c.state !== 'flushing' && <progress max={c.total} value={c.done} aria-label="Frames captured" />}
      {c.state !== 'flats' && c.buffer_capacity > 0 && (
        <>
          <progress max={c.buffer_capacity} value={c.buffer_queued} className="buf" aria-label="RAM buffer in use" />
          <div className="hint-text">RAM buffer {c.buffer_queued}/{c.buffer_capacity} frames · disk writing {c.disk_mb_s.toFixed(0)} MB/s</div>
        </>
      )}
      <div className="hint-text">{c.done}{c.total != null ? ` / ${c.total}` : ''} frames {c.file && `· ${c.file}`}</div>
      <div className="btnrow">
        {['sequence', 'bursts'].includes(c.state) && <button type="button" className="grow big" onClick={() => { api('POST', '/capture/pause').catch(() => {}) }}>Pause</button>}
        {active && <button type="button" className="danger grow big" onClick={onStop}>Stop recording</button>}
      </div>
    </div>
  )
}

/** Step 4: record. One recording form: what to record (plain, with a prominence lane, or HDR lanes), then how much and how often. */
export function RecordTab({ status, goto, openFiles }: { status: Status | null; goto: (t: 'more', focus: string) => void; openFiles: () => void }) {
  const [name, setName] = useSetting('capture.name', '')
  const [kind, setKind] = useSetting<'plain' | 'prominence' | 'hdr'>('capture.kind', 'plain')
  const [format, setFormat] = useSetting<'ser' | 'png' | 'fits'>('capture.format', 'ser')
  const [onFull, setOnFull] = useSetting<'wait' | 'drop'>('capture.on_full', 'wait')
  const [frames, setFrames] = useSetting('capture.frames', '100')
  const [seconds, setSeconds] = useSetting('capture.seconds', '')
  const [every, setEvery] = useSetting('capture.every', '')
  const [repeats, setRepeats] = useSetting('capture.bursts', '')
  const [factor, setFactor] = useSetting('capture.promvid_factor', '8')
  const [mask, setMask] = useSetting('capture.promvid_mask', false)
  const [home, setHome] = useSetting('capture.home_when_done', false)
  const [lanesJson, setLanesJson] = useSetting('capture.lanes', JSON.stringify(PRESETS['Partial phase']))
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

  if (c.paused) {
    return (
      <Section title="Recording paused" error={err}>
        <p className="hint-text">{c.paused_reason ?? 'Paused'}. {c.paused_reason === 'paused by you' ? 'Resume carries on with what was left (new files in the same style, named "-resumed"), or discard it.' : 'It starts again by itself when the flip is done, or discard it.'}</p>
        {c.paused_reason === 'paused by you' && <button type="button" className="wide big" onClick={() => run(() => api('POST', '/capture/resume'))}>Resume recording</button>}
        <button type="button" className="danger wide big" onClick={() => run(() => api('POST', '/capture/stop'))}>Discard</button>
      </Section>
    )
  }

  const lanes = parseLanes(lanesJson)
  const exp = cam.controls.find((x) => x.id === 'exposure_us')?.value
  const lanesMode = kind !== 'plain'
  const repeating = every.trim() !== ''

  // planning aid: how big is one recording, does it fit in the RAM buffer, is the disk likely to keep up?
  const frameBytes = cam.width * cam.height * (cam.bits / 8)
  const nFrames = kind === 'hdr' ? Math.max(...lanes.map((l) => parseInt(l.frames, 10) || 0)) : (n(frames) ?? (n(seconds) && cam.fps ? n(seconds)! * cam.fps : 0))
  const burstBytes = nFrames * frameBytes
  const bufFrames = frameBytes ? Math.floor(((c.buffer_mb ?? 1024) * 1048576) / frameBytes) : 0
  const burstSecs = cam.fps ? nFrames / cam.fps : 0
  const every_s = n(every) ?? 0
  const diskNeed = burstSecs && every_s ? burstBytes / Math.max(every_s, burstSecs) / 1e6 : 0

  // when will it end? Needs a known count of repeats or cycles (or a single recording)
  const fps = cam.fps > 0 ? cam.fps : 30
  const laneSecs = (l: { exposure_ms?: string; frames?: string; seconds?: string }, expUs: number) => {
    const sec = parseFloat(l.seconds ?? '')
    if (sec > 0) return sec
    const fr = parseInt(l.frames ?? '', 10) || 0
    return (fr + 2) * Math.max(expUs / 1e6, 1 / fps) // two frames after each exposure change are thrown away
  }
  const reps = n(repeats)
  let planSecs = 0
  if (kind === 'plain') {
    if (burstSecs > 0) planSecs = repeating ? (reps ? (reps - 1) * Math.max(every_s, burstSecs) + burstSecs : 0) : burstSecs
  } else {
    const specs = kind === 'hdr'
      ? lanes.map((l) => ({ us: (parseFloat(l.exposure_ms) || 0) * 1000, frames: l.frames }))
      : [{ us: exp ?? 0, frames: frames }, { us: (exp ?? 0) * (parseFloat(factor) || 8), frames: frames }]
    const cycle = specs.reduce((a, l) => a + laneSecs({ frames: l.frames, seconds }, l.us), 0)
    if (cycle > 0 && reps) planSecs = (reps - 1) * Math.max(every_s, cycle) + cycle
  }

  const start = () => {
    const q = recordRequest({ kind, name, format, onFull, frames, seconds, every, repeats, factor, mask, lanes, home }, cam)
    return run(() => api('POST', q.path, q.body))
  }

  return (
    <>
      <Section title="Record" error={err}>
        <Segmented label="What to record" value={kind} onChange={setKind} options={[{ id: 'plain', label: 'Sequence' }, { id: 'prominence', label: '+ Prominence' }, { id: 'hdr', label: 'HDR lanes' }]} />
        <p className="hint-text">
          {kind === 'plain' && 'Frames at the current exposure, once or repeated.'}
          {kind === 'prominence' && `Two lanes alternate: the disc at the current exposure${exp ? ` (${(exp / 1000).toFixed(2)} ms)` : ''} and a longer exposure for the prominences. Each lane is its own raw video in its own folder.`}
          {kind === 'hdr' && 'Several exposures in turn, each in its own lane and folder, for eclipses and the Moon. Every lane is raw video.'}
        </p>
        <Field label="Name (optional)">{(id) => <input id={id} value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. AR3842" />}</Field>

        {kind === 'hdr' ? (
          <LaneEditor lanes={lanes} onChange={(l) => setLanesJson(JSON.stringify(l))} />
        ) : (
          <div className="grid2">
            <Field label={kind === 'prominence' ? 'Frames per lane' : 'Frames'}>{(id) => <input id={id} inputMode="numeric" value={frames} onChange={(e) => setFrames(e.target.value)} />}</Field>
            <Field label="or seconds">{(id) => <input id={id} inputMode="decimal" value={seconds} onChange={(e) => setSeconds(e.target.value)} placeholder="stop after…" />}</Field>
          </div>
        )}
        {kind === 'prominence' && (
          <>
            <Field label="Prominence exposure, times the disc">{(id) => <input id={id} inputMode="decimal" value={factor} onChange={(e) => setFactor(e.target.value)} />}</Field>
            <Switch label="Coronagraph: black out the disc in the prominence lane" checked={mask} onChange={setMask} hint={mask ? 'The saved frames of that lane have the disc masked (not raw there). Needs the Sun in view; the disc lane stays raw.' : 'Off: both lanes are saved raw. Turn on to record the prominence lane with the disc hidden.'} />
          </>
        )}
        <div className="grid2">
          <Field label={lanesMode ? 'Repeat a cycle every (s)' : 'Repeat every (s)'}>{(id) => <input id={id} inputMode="decimal" value={every} onChange={(e) => setEvery(e.target.value)} placeholder={lanesMode ? 'back to back' : 'once only'} />}</Field>
          {(repeating || lanesMode) && <Field label={lanesMode ? 'Number of cycles' : 'Number of repeats'}>{(id) => <input id={id} inputMode="numeric" value={repeats} onChange={(e) => setRepeats(e.target.value)} placeholder="until stopped" />}</Field>}
        </div>
        {nFrames > 0 && (
          <p className="hint-text plan">
            {lanesMode ? 'Largest lane' : repeating ? 'Each repeat' : 'This recording'}: ≈ {Math.round(nFrames)} frames, {gb(burstBytes)}{burstSecs ? `, ≈ ${burstSecs.toFixed(1)} s at ${cam.fps.toFixed(0)} fps` : ''}.
            {bufFrames > 0 && (nFrames <= bufFrames
              ? ` Fits in the RAM buffer (${bufFrames} frames), so the disk can be slower than the camera.`
              : ` Larger than the RAM buffer (${bufFrames} frames): once it is full, capture slows to disk speed.`)}
            {diskNeed > 0 && ` Average disk load ≈ ${diskNeed.toFixed(0)} MB/s.`}
            {planSecs > 0 && <> <b>Ends {endsText(planSecs)}</b> if started now.</>}
          </p>
        )}
        {planSecs === 0 && (repeating || lanesMode) && !reps && <p className="hint-text">Give a number of {lanesMode ? 'cycles' : 'repeats'} to see when it will end; left empty it runs until you stop it.</p>}
        <Switch label="Send the mount home when the recording finishes" checked={home} disabled={!status?.mount} onChange={setHome}
          hint={!status?.mount ? 'Connect the mount first (Aim tab).' : home ? 'Only when it finishes by itself (a set number of repeats or cycles, or a frame or time limit). Guiding stops and the mount slews to its home position: keep the cables and counterweight clear. Stopping the recording, an error or a pause does not move the mount.' : 'Off: the mount stays where it is.'} />
        <Segmented label="File format" value={format} onChange={setFormat} options={lanesMode ? [{ id: 'ser', label: 'SER video' }, { id: 'fits', label: 'FITS' }] : [{ id: 'ser', label: 'SER video' }, { id: 'png', label: 'PNG' }, { id: 'fits', label: 'FITS' }]} />
        <Segmented label="When the RAM buffer is full" value={onFull} onChange={setOnFull} options={[{ id: 'wait', label: 'Wait for the disk' }, { id: 'drop', label: 'Drop frames' }]} />
        <div className="hint-text">{onFull === 'drop' ? 'Keeps the camera at full speed; frames that do not fit are dropped and counted.' : 'Never loses a frame: a camera faster than the disk slows to disk speed once the buffer is full.'}{format === 'fits' ? ' FITS makes one file per frame, with exposure, gain and time in the header.' : ''}</div>
        <StorageLine status={status} onChange={() => goto('more', 'storage')} />
        <div className="sticky-act"><button type="button" className="primary wide big" onClick={start}>{kind === 'plain' ? (repeating ? 'Start repeating recording' : 'Start recording') : kind === 'prominence' ? 'Start prominence recording' : 'Start HDR lanes'}</button></div>
      </Section>
      {c.last_file && <div className="hint-text lastfile">Last: <a href={`/files/${c.last_file}`}>{c.last_file}</a> ({c.phase}{c.disk_mb_s > 0 ? `, written at ${c.disk_mb_s.toFixed(0)} MB/s` : ''})</div>}
      <Disclosure title="Stills" summary="single frame, prominence pair">
        <button type="button" className="wide" onClick={() => run(() => api('POST', '/capture/single', { name: nm }))}>Save a single frame (PNG)</button>
        <p className="hint-text">Prominence pair: the disc and a much longer exposure as PNGs plus a composite (processed, for a quick look; use the + Prominence recording for raw data).</p>
        <ProminencePair status={status} />
      </Disclosure>
      <FileManagerButton status={status} onOpen={openFiles} />
    </>
  )
}
