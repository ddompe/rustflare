import { useEffect, useState } from 'react'
import { AfterBurst as AfterBurstState, AfterMode, api, Status } from '../api'
import { Disclosure, ErrorLine, Field, Switch, useAction } from './ui'
import { useStackInfo } from './Stacking'
import { whyStack } from '../profile'

const MODES: { id: AfterMode; title: string; text: string; warn?: string }[] = [
  { id: 'video', title: 'Keep the video', text: 'Write every frame. Stack later from the file if you want to.' },
  { id: 'video_stack', title: 'Video and stack', text: 'Write every frame and also stack each burst from memory.' },
  { id: 'stack_only', title: 'Stack only', text: 'Stack each burst from memory and write no video. Smallest on disk.', warn: 'The raw frames are not kept' },
  { id: 'best_frames', title: 'Best frames only', text: 'Stack from memory and keep only the best frames of each burst as a short video.' },
]

const gb = (b: number) => (b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : `${Math.max(1, Math.round(b / 1e6))} MB`)

/**
 * What happens to each burst once it is captured. The plan line says what it costs on disk and whether the stacker can keep up
 * with the gaps between bursts; a mode that cannot be honoured is never quiet (the status bar says so, until dismissed).
 */
export function AfterBurst({ status, nFrames, frameBytes, burstSecs, everySecs, bufFrames, onMoreStacking }: { status: Status; nFrames: number; frameBytes: number; burstSecs: number; everySecs: number; bufFrames: number; onMoreStacking?: () => void }) {
  const a: AfterBurstState | undefined = status.after_burst
  const { err, run } = useAction()
  const [stackInfo, reloadStack] = useStackInfo()
  const sc = stackInfo?.config
  const saveStack = (patch: Record<string, unknown>) => run(async () => { await api('PUT', '/stack/config', patch); reloadStack() })
  const [diskKeep, setDiskKeep] = useState('')
  useEffect(() => { if (sc) setDiskKeep(String(sc.keep_percent)) }, [sc?.keep_percent]) // eslint-disable-line react-hooks/exhaustive-deps
  const [keep, setKeep] = useState('')
  const [raw, setRaw] = useState('')
  useEffect(() => { if (a) { setKeep(String(a.stack_keep_percent)); setRaw(String(a.raw_keep_percent)) } }, [a?.stack_keep_percent, a?.raw_keep_percent]) // eslint-disable-line react-hooks/exhaustive-deps
  if (!a) return null
  const cam = status.camera
  const set = (patch: Record<string, unknown>) => run(() => api('PUT', '/capture/after-burst', patch))
  const stacking = a.mode !== 'video'
  const noStack = whyStack(status)
  const rawFrac = a.mode === 'video' || a.mode === 'video_stack' ? 1 : a.mode === 'best_frames' ? a.raw_keep_percent / 100 : 0
  const stackBytes = stacking && cam ? cam.width * cam.height * 4 : 0
  const burstBytes = nFrames * frameBytes * rawFrac + stackBytes
  const perHour = everySecs > 0 && burstSecs > 0 ? (3600 / Math.max(everySecs, burstSecs)) * burstBytes : 0
  const stackSecs = nFrames * a.stack_s_per_frame
  const gap = everySecs > 0 ? everySecs - burstSecs : 0

  return (
    <Disclosure title="After each burst" summary={MODES.find((m) => m.id === a.mode)?.title}>
      <p className="hint-text">A burst is one run of frames, the number you set above. This is what happens to it once it is captured.</p>
      <div className="mode-cards" role="group" aria-label="After each burst">
        {MODES.map((m) => (
          <button key={m.id} type="button" className="mode-card" aria-pressed={a.mode === m.id} disabled={!!noStack && m.id !== 'video' && a.mode !== m.id} onClick={() => set({ mode: m.id })}>
            <b>{m.title}</b><span>{m.text}</span>{m.warn && <em className="mode-warn">{m.warn}</em>}
          </button>
        ))}
      </div>
      {noStack && <div className="gate-why hint-text" role="note">{noStack}</div>}
      {stacking && (
        <div className="grid2">
          <Field label="Best frames kept, stacked from memory (%)">{(id) => <input id={id} inputMode="decimal" value={keep} onChange={(e) => setKeep(e.target.value)} onBlur={() => { const v = parseFloat(keep); if (v !== a.stack_keep_percent) void set({ stack_keep_percent: v }) }} />}</Field>
          {a.mode === 'best_frames' && <Field label="Best frames kept as video (%)">{(id) => <input id={id} inputMode="decimal" value={raw} onChange={(e) => setRaw(e.target.value)} onBlur={() => { const v = parseFloat(raw); if (v !== a.raw_keep_percent) void set({ raw_keep_percent: v }) }} />}</Field>}
        </div>
      )}
      {stacking && (
        <p className={`hint-text ${a.stacker_connected ? '' : 'err'}`} role={a.stacker_connected ? undefined : 'alert'}>
          {a.stacker_connected
            ? `Stacker ready${a.stacking ? `, ${a.stacking} burst(s) being stacked` : ''}${a.last ? `. Last: ${a.last}` : ''}.`
            : 'The stacker is not running, so bursts are saved as video for now. RustFlare tries to restart it by itself; if it stays off, restart the Pi (More > Stacking has details).'}
        </p>
      )}
      {nFrames > 0 && stacking && (
        <p className="hint-text plan">
          Each burst: {rawFrac > 0 ? `${gb(nFrames * frameBytes * rawFrac)} of video + ` : 'no video, '}one stack of {gb(stackBytes)}{perHour > 0 ? `; about ${gb(perHour)} an hour` : ''}.
          {` A stack takes about ${stackSecs.toFixed(stackSecs < 10 ? 1 : 0)} s`}{gap > 0 ? (stackSecs * a.margin <= gap ? `; the gap between bursts is ${gap.toFixed(0)} s, so it keeps up.` : `; the gap between bursts is only ${gap.toFixed(0)} s, so stacks will queue and a burst may be written as video instead.`) : '.'}
          {bufFrames > 0 && nFrames + 8 > bufFrames ? ` A burst of ${Math.round(nFrames)} frames does not fit in the RAM buffer (${bufFrames} frames): it will be recorded as video.` : ''}
        </p>
      )}
      {sc && (
        <Disclosure title="Stacker settings" summary={`${sc.enabled ? 'on' : 'off'} · ${sc.multipoint ? 'multi-point' : 'global'} · ${sc.disc_mask ? `black disc ${Math.round(sc.disc_scale * 100)} %` : 'disc kept'}`}>
          <Switch label="Stack finished recordings" checked={sc.enabled} onChange={(v) => void saveStack({ enabled: v })} hint="Off also switches off stacking from memory." />
          <Switch label="Multi-point alignment" checked={sc.multipoint} onChange={(v) => void saveStack({ multipoint: v })} hint="A shift for each patch of the picture: sharper, a little slower." />
          <Field label="Best frames kept, stacked from a video file (%)" hint="Used for recordings stacked after they finish. Bursts stacked from memory use the percentage above.">{(id) => <input id={id} inputMode="decimal" value={diskKeep} onChange={(e) => setDiskKeep(e.target.value)} onBlur={() => { const v = parseFloat(diskKeep); if (v >= 1 && v <= 100 && v !== sc.keep_percent) void saveStack({ keep_percent: v }) }} />}</Field>
          <Switch label="Black disc on prominence stacks" checked={sc.disc_mask} onChange={(v) => void saveStack({ disc_mask: v })} hint="For a prominence lane recorded with the coronagraph: the disc is found in the finished stack and covered; off keeps it as recorded." />
          {sc.disc_mask && <Field label="Black disc size (% of the Sun's radius)" hint="100 is the edge as fitted; a few per cent more hides the limb glow and the base of a prominence.">{(id) => <input id={id} inputMode="numeric" defaultValue={Math.round(sc.disc_scale * 100)} key={sc.disc_scale} onBlur={(e) => { const n = parseFloat(e.target.value); if (n >= 90 && n <= 120 && Math.abs(n / 100 - sc.disc_scale) > 1e-6) void saveStack({ disc_scale: n / 100 }) }} />}</Field>}
          {onMoreStacking && <button type="button" className="quiet small" onClick={onMoreStacking}>More: when to stack, flat and darks (More &gt; Stacking)</button>}
        </Disclosure>
      )}
      <Disclosure title="Safety margin" summary={`${a.margin}×`}>
        <Field label="Plan a stack to take this many times its measured time" hint="If a stack might not finish before the camera overwrites the burst in memory, the burst is written as video instead. A larger margin gives up sooner.">{(id) => <input id={id} inputMode="decimal" defaultValue={a.margin} onBlur={(e) => { const v = parseFloat(e.target.value); if (v !== a.margin) void set({ margin: v }) }} />}</Field>
      </Disclosure>
      <ErrorLine error={err} />
    </Disclosure>
  )
}
