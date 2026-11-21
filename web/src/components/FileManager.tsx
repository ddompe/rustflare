import { useEffect, useState } from 'react'
import { api, Status } from '../api'
import { Disclosure, ErrorLine, useAction } from './ui'

interface FileEntry { path: string; size: number; modified: number | null; folder_files: number | null }
interface Stats {
  width: number; height: number; depth: number; frames: number; frames_present: number; complete: boolean
  fps: number | null; duration_s: number | null; mean_min: number; mean_avg: number; mean_max: number
  sampled: number; dark_fraction: number; mostly_dark: boolean
}

const size = (b: number) => (b > 1e9 ? `${(b / 1e9).toFixed(2)} GB` : b > 1e6 ? `${(b / 1e6).toFixed(1)} MB` : `${Math.max(1, Math.round(b / 1e3))} kB`)
const when = (s: number | null) => (s ? new Date(s * 1000).toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' }) : '')
const isSer = (p: string) => p.toLowerCase().endsWith('.ser')

function Row({ f, stats, busy, onStats, onDeleted }: { f: FileEntry; stats?: Stats | { error: string }; busy: boolean; onStats: () => void; onDeleted: () => void }) {
  const [ask, setAsk] = useState(false)
  const { err, run } = useAction()
  const st = stats && !('error' in stats) ? stats : undefined
  return (
    <li className="fm-row">
      <div className="fm-head">
        <span className="fm-name">{f.path}{f.folder_files != null ? '/' : ''}</span>
        {st?.mostly_dark && <span className="tag warn" title={`${Math.round(st.dark_fraction * 100)}% of sampled frames are dark`}>Mostly dark</span>}
        {st && !st.complete && <span className="tag warn" title="The file ends early: the recording was cut off">Incomplete</span>}
      </div>
      <div className="muted fm-sub">
        {f.folder_files != null ? `${f.folder_files} frames · ` : ''}{size(f.size)}{f.modified ? ` · ${when(f.modified)}` : ''}
        {st && <> · {st.width}×{st.height} · {st.depth}-bit · {st.frames_present}/{st.frames} frames{st.fps ? ` · ${st.fps.toFixed(1)} fps` : ''}{st.duration_s ? ` · ${st.duration_s.toFixed(1)} s` : ''} · brightness {Math.round(st.mean_avg * 100)}% (min {Math.round(st.mean_min * 100)}, max {Math.round(st.mean_max * 100)})</>}
        {stats && 'error' in stats && <> · {stats.error}</>}
      </div>
      {!ask ? (
        <div className="btnrow tight fm-actions">
          {isSer(f.path) && <button type="button" className="small" onClick={onStats}>{stats ? 'Re-check' : 'Check'}</button>}
          {f.folder_files == null && <a className="btn small" href={`/files/${f.path}`} download>Download</a>}
          <button type="button" className="small" disabled={busy} onClick={() => setAsk(true)}>Delete</button>
        </div>
      ) : (
        <div className="confirm" role="alertdialog" aria-label={`Delete ${f.path}`}>
          <p>Delete {f.path} permanently?</p>
          <div className="btnrow tight">
            <button type="button" className="grow" onClick={() => setAsk(false)}>Keep</button>
            <button type="button" className="danger grow" onClick={() => run(async () => { await api('POST', '/files/delete', { path: f.path, confirm: true }); onDeleted() })}>Delete</button>
          </div>
        </div>
      )}
      <ErrorLine error={err} />
    </li>
  )
}

/** Review, check and delete captures: sizes, SER timing, brightness, and a flag for recordings that are mostly dark. */
export function FileManager({ status }: { status: Status | null }) {
  const [files, setFiles] = useState<FileEntry[]>([])
  const [stats, setStats] = useState<Record<string, Stats | { error: string }>>({})
  const [checking, setChecking] = useState(false)
  const last = status?.capture.last_file
  const busy = !!status && status.capture.state !== 'idle'
  const load = () => api<FileEntry[]>('GET', '/files').then(setFiles).catch(() => {})
  useEffect(() => { load() }, [last, busy]) // eslint-disable-line react-hooks/exhaustive-deps
  const check = async (p: string) => {
    try { const s = await api<Stats>('GET', `/files/stats?path=${encodeURIComponent(p)}`); setStats((x) => ({ ...x, [p]: s })) }
    catch (e) { setStats((x) => ({ ...x, [p]: { error: e instanceof Error ? e.message : String(e) } })) }
  }
  const checkAll = async () => { setChecking(true); for (const f of files.filter((f) => isSer(f.path))) await check(f.path); setChecking(false) }
  const total = files.reduce((a, f) => a + f.size, 0)
  const dark = Object.values(stats).filter((s): s is Stats => !('error' in s) && s.mostly_dark).length
  return (
    <Disclosure title="Captures" summary={files.length ? `${files.length} · ${size(total)}${dark ? ` · ${dark} dark` : ''}` : 'none yet'}>
      {files.length === 0 && <span className="muted">Nothing captured yet</span>}
      {files.some((f) => isSer(f.path)) && <button type="button" className="wide" disabled={checking || busy} onClick={checkAll}>{checking ? 'Checking…' : 'Check all recordings (brightness, timing, cut-offs)'}</button>}
      <ul className="files fm">
        {files.map((f) => <Row key={f.path} f={f} stats={stats[f.path]} busy={busy} onStats={() => check(f.path)} onDeleted={() => { setStats(({ [f.path]: _drop, ...rest }) => rest); load() }} />)}
      </ul>
      {busy && <div className="hint-text">Deleting is disabled while recording or writing.</div>}
    </Disclosure>
  )
}
