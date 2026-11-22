import { api, Status } from '../api'
import { ErrorLine, Switch, useAction } from './ui'
import { useSetting } from '../settings'

const mb = (b: number) => `${(b / 1e6).toFixed(1)} MB`

/** Opt-in session log: what the tracker and guider saw, once a second, plus small frames, for building regression tests. */
export function SessionLog({ status }: { status: Status | null }) {
  const [every, setEvery] = useSetting('log.frame_every', '10')
  const { err, run } = useAction()
  const l = status?.session_log
  return (
    <div>
      <Switch label="Save the frame when tracking is lost" checked={l?.lost_frames ?? true} onChange={(on) => run(() => api('PUT', '/debug/lost-frames', { enabled: on }))}
        hint={`On by default. Keeps the picture and a note (exposure, gain, lane, mask, last Sun position) in logs/<date>/lost on the capture drive, at most one every 10 s. ${l?.lost_saved ? `${l.lost_saved} saved since the service started.` : 'None saved yet.'}`} />
      <Switch label="Record a session log" checked={!!l?.enabled} onChange={(on) => run(() => api('PUT', '/session-log', { enabled: on, frame_every_s: every === '0' ? null : parseFloat(every) }))}
        hint={l?.enabled ? `${l.lines} lines, ${l.frames} frames, ${mb(l.bytes)} in ${l.file}` : 'Off. When on, one line per second (camera level, detected Sun, tracking, guiding error, guide pulses) goes into the logs folder on the capture drive.'} />
      <label className="field"><span className="hint-text">Save a small picture every</span>
        <select value={every} disabled={!!l?.enabled} onChange={(e) => setEvery(e.target.value)}>
          <option value="0">never</option><option value="5">5 s</option><option value="10">10 s</option><option value="30">30 s</option><option value="60">60 s</option>
        </select></label>
      <p className="hint-text">Nothing identifying is logged (no site, device names or file paths). It stops by itself at 200 MB. Copy the <code>logs</code> folder from the drive and run <code>RUSTFLARE_FIXTURES=&lt;folder&gt; cargo test -p rustflare-server replay_real -- --nocapture</code> to replay the detector on it.</p>
      <ErrorLine error={err} />
    </div>
  )
}
