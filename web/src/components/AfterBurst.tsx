import { useEffect, useState } from 'react'
import { AfterBurst as AfterBurstState, AfterMode, api, Status } from '../api'
import { Disclosure, ErrorLine, Field, useAction } from './ui'

const MODES: { id: AfterMode; title: string; text: string }[] = [
  { id: 'video', title: 'Keep the video', text: 'Write every frame. Stack later from the file if you want to.' },
  { id: 'video_stack', title: 'Video and stack', text: 'Write every frame and also stack each burst from memory.' },
  { id: 'stack_only', title: 'Stack only', text: 'Stack each burst from memory and write no video. Smallest on disk; the raw frames are not kept.' },
  { id: 'best_frames', title: 'Best frames only', text: 'Stack from memory and keep only the best frames of each burst as a short video.' },
]

const gb = (b: number) => (b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : `${Math.max(1, Math.round(b / 1e6))} MB`)

/**
 * What happens to each burst once it is captured. The plan line says what it costs on disk and whether the stacker can keep up
 * with the gaps between bursts; a mode that cannot be honoured is never quiet (the status bar says so, until dismissed).
 */
export function AfterBurst({ status, nFrames, frameBytes, burstSecs, everySecs, bufFrames }: { status: Status; nFrames: number; frameBytes: number; burstSecs: number; everySecs: number; bufFrames: number }) {
  const a: AfterBurstState | undefined = status.after_burst
  const { err, run } = useAction()
  const [keep, setKeep] = useState('')
  const [raw, setRaw] = useState('')
  useEffect(() => { if (a) { setKeep(String(a.stack_keep_percent)); setRaw(String(a.raw_keep_percent)) } }, [a?.stack_keep_percent, a?.raw_keep_percent]) // eslint-disable-line react-hooks/exhaustive-deps
  if (!a) return null
  const cam = status.camera
  const set = (patch: Record<string, unknown>) => run(() => api('PUT', '/capture/after-burst', patch))
  const stacking = a.mode !== 'video'
  const rawFrac = a.mode === 'video' || a.mode === 'video_stack' ? 1 : a.mode === 'best_frames' ? a.raw_keep_percent / 100 : 0
  const stackBytes = stacking && cam ? cam.width * cam.height * 4 : 0
  const burstBytes = nFrames * frameBytes * rawFrac + stackBytes
  const perHour = everySecs > 0 && burstSecs > 0 ? (3600 / Math.max(everySecs, burstSecs)) * burstBytes : 0
  const stackSecs = nFrames * a.stack_s_per_frame
  const gap = everySecs > 0 ? everySecs - burstSecs : 0

  return (
    <Disclosure title="After each burst" summary={MODES.find((m) => m.id === a.mode)?.title}>
      <div className="mode-cards" role="group" aria-label="After each burst">
        {MODES.map((m) => (
          <button key={m.id} type="button" className="mode-card" aria-pressed={a.mode === m.id} onClick={() => set({ mode: m.id })}>
            <b>{m.title}</b><span>{m.text}</span>
          </button>
        ))}
      </div>
      {stacking && (
        <div className="grid2">
          <Field label="Frames in each stack (%)">{(id) => <input id={id} inputMode="decimal" value={keep} onChange={(e) => setKeep(e.target.value)} onBlur={() => { const v = parseFloat(keep); if (v !== a.stack_keep_percent) void set({ stack_keep_percent: v }) }} />}</Field>
          {a.mode === 'best_frames' && <Field label="Frames kept as video (%)">{(id) => <input id={id} inputMode="decimal" value={raw} onChange={(e) => setRaw(e.target.value)} onBlur={() => { const v = parseFloat(raw); if (v !== a.raw_keep_percent) void set({ raw_keep_percent: v }) }} />}</Field>}
        </div>
      )}
      {stacking && (
        <p className={`hint-text ${a.stacker_connected ? '' : 'err'}`} role={a.stacker_connected ? undefined : 'alert'}>
          {a.stacker_connected
            ? `Stacker ready${a.stacking ? `, ${a.stacking} burst(s) being stacked` : ''}${a.last ? `. Last: ${a.last}` : ''}.`
            : 'The stacker is not running, so bursts are recorded as video. Start it with “systemctl start rustflare-stack” (More > Stacking).'}
        </p>
      )}
      {nFrames > 0 && stacking && (
        <p className="hint-text plan">
          Each burst: {rawFrac > 0 ? `${gb(nFrames * frameBytes * rawFrac)} of video + ` : 'no video, '}one stack of {gb(stackBytes)}{perHour > 0 ? `; about ${gb(perHour)} an hour` : ''}.
          {` A stack takes about ${stackSecs.toFixed(stackSecs < 10 ? 1 : 0)} s`}{gap > 0 ? (stackSecs * a.margin <= gap ? `; the gap between bursts is ${gap.toFixed(0)} s, so it keeps up.` : `; the gap between bursts is only ${gap.toFixed(0)} s, so stacks will queue and a burst may be written as video instead.`) : '.'}
          {bufFrames > 0 && nFrames + 8 > bufFrames ? ` A burst of ${Math.round(nFrames)} frames does not fit in the RAM buffer (${bufFrames} frames): it will be recorded as video.` : ''}
        </p>
      )}
      <Disclosure title="Safety margin" summary={`${a.margin}×`}>
        <Field label="Plan a stack to take this many times its measured time" hint="If a stack might not finish before the camera overwrites the burst in memory, the burst is written as video instead. A larger margin gives up sooner.">{(id) => <input id={id} inputMode="decimal" defaultValue={a.margin} onBlur={(e) => { const v = parseFloat(e.target.value); if (v !== a.margin) void set({ margin: v }) }} />}</Field>
      </Disclosure>
      <ErrorLine error={err} />
    </Disclosure>
  )
}
