import { useEffect, useMemo, useRef, useState } from 'react'
import { api, Status } from '../api'
import { ErrorLine } from './ui'
import { useStorage } from './Storage'

interface LaneInfo { name: string; size: number; files: { path: string; size: number }[] }
interface FileEntry { path: string; size: number; modified: number | null; folder_files: number | null; kind?: 'hdr' | 'prominence' | null; lanes?: LaneInfo[] }
const KIND_LABEL: Record<string, string> = { hdr: 'HDR lanes', prominence: 'Prominence video' }
interface Stats {
  width: number; height: number; depth: number; frames: number; frames_present: number; complete: boolean
  fps: number | null; duration_s: number | null; mean_min: number; mean_avg: number; mean_max: number
  sampled: number; dark_fraction: number; mostly_dark: boolean
}
type StatsOrError = Stats | { error: string }

const size = (b: number) => (b > 1e12 ? `${(b / 1e12).toFixed(2)} TB` : b > 1e9 ? `${(b / 1e9).toFixed(2)} GB` : b > 1e6 ? `${(b / 1e6).toFixed(1)} MB` : `${Math.max(1, Math.round(b / 1e3))} kB`)
const time = (s: number | null) => (s ? new Date(s * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) : '')
const isSer = (p: string) => p.toLowerCase().endsWith('.ser')
const base = (p: string) => p.split('/').slice(1).join('/')
const day = (p: string) => p.split('/')[0]

/** The captures list without the .json notes files and the .ready markers (they travel with their capture). */
function useFiles(refreshKey: unknown) {
  const [all, setAll] = useState<FileEntry[]>([])
  const load = () => api<FileEntry[]>('GET', '/files').then(setAll).catch(() => {})
  useEffect(() => { load() }, [refreshKey]) // eslint-disable-line react-hooks/exhaustive-deps
  const files = all.filter((f) => !f.path.endsWith('.ready') && !(f.path.endsWith('.json') && all.some((o) => o.path !== f.path && o.path.startsWith(f.path.slice(0, -5)))))
  return { files, load }
}

/** Record-tab entry point: a summary line and the button that opens the full-screen manager. */
export function FileManagerButton({ status, onOpen }: { status: Status | null; onOpen: () => void }) {
  const { files } = useFiles(status?.capture.last_file)
  const total = files.reduce((a, f) => a + f.size, 0)
  return (
    <button type="button" className="wide" onClick={onOpen}>
      Open file manager · {files.length ? `${files.length} captures, ${size(total)}` : 'nothing captured yet'}
    </button>
  )
}

/** Full-screen file manager: browse by day, check recordings (brightness, timing, cut-offs), download, delete in bulk. */
export function FileManagerScreen({ status, onClose }: { status: Status | null; onClose: () => void }) {
  const busy = !!status && status.capture.state !== 'idle'
  const { files, load } = useFiles(`${status?.capture.last_file}|${busy}`)
  const { data: storage } = useStorage()
  const [stats, setStats] = useState<Record<string, StatsOrError>>({})
  const [sel, setSel] = useState<Set<string>>(new Set())
  const [sort, setSort] = useState<'new' | 'big' | 'name'>('new')
  const [show, setShow] = useState<'all' | 'rec' | 'hdr' | 'prominence'>('all')
  const [checking, setChecking] = useState(false)
  const [ask, setAsk] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const closeRef = useRef<HTMLButtonElement>(null)

  useEffect(() => {
    closeRef.current?.focus()
    const esc = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose() }
    document.addEventListener('keydown', esc)
    const prev = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    return () => { document.removeEventListener('keydown', esc); document.body.style.overflow = prev }
  }, [onClose])

  const check = async (p: string) => {
    try { const s = await api<Stats>('GET', `/files/stats?path=${encodeURIComponent(p)}`); setStats((x) => ({ ...x, [p]: s })) }
    catch (e) { setStats((x) => ({ ...x, [p]: { error: e instanceof Error ? e.message : String(e) } })) }
  }
  const checkAll = async () => { setChecking(true); for (const f of files.filter((f) => isSer(f.path))) await check(f.path); setChecking(false) }
  const toggle = (p: string) => setSel((s) => { const n = new Set(s); if (n.has(p)) n.delete(p); else n.add(p); return n })

  const sorted = useMemo(() => {
    const f = files.filter((x) => show === 'all' || (show === 'rec' ? !x.kind : x.kind === show))
    if (sort === 'big') f.sort((a, b) => b.size - a.size)
    else if (sort === 'name') f.sort((a, b) => a.path.localeCompare(b.path))
    else f.sort((a, b) => (b.modified ?? 0) - (a.modified ?? 0))
    return f
  }, [files, sort, show])
  const groups = useMemo(() => {
    if (sort !== 'new' && sort !== 'name') return [['All captures', sorted] as [string, FileEntry[]]]
    const m = new Map<string, FileEntry[]>()
    for (const f of sorted) m.set(day(f.path), [...(m.get(day(f.path)) ?? []), f])
    return [...m.entries()]
  }, [sorted, sort])

  const total = files.reduce((a, f) => a + f.size, 0)
  const dark = files.filter((f) => { const s = stats[f.path]; return s && !('error' in s) && s.mostly_dark })
  const cut = files.filter((f) => { const s = stats[f.path]; return s && !('error' in s) && !s.complete })
  const selSize = files.filter((f) => sel.has(f.path)).reduce((a, f) => a + f.size, 0)
  const free = storage?.current.free

  const del = async () => {
    setErr(null)
    for (const p of sel) {
      try { await api('POST', '/files/delete', { path: p, confirm: true }) }
      catch (e) { setErr(`${p}: ${e instanceof Error ? e.message : e}`); break }
      setSel((s) => { const n = new Set(s); n.delete(p); return n })
      setStats(({ [p]: _gone, ...rest }) => rest)
    }
    setAsk(false)
    load()
  }

  return (
    <div className="fm-screen" role="dialog" aria-modal="true" aria-label="File manager">
      <header className="fm-top">
        <h2>Captures</h2>
        <div className="fm-summary muted">
          {files.length} files · {size(total)}{free != null ? ` · ${size(free)} free` : ''}
          {dark.length > 0 && <> · <b className="warn-text">{dark.length} mostly dark</b></>}
          {cut.length > 0 && <> · <b className="warn-text">{cut.length} incomplete</b></>}
        </div>
        <button ref={closeRef} type="button" className="fm-close" onClick={onClose} aria-label="Close file manager">✕</button>
      </header>

      <div className="fm-bar">
        <button type="button" disabled={checking || busy || !files.some((f) => isSer(f.path))} onClick={checkAll}>{checking ? 'Checking…' : 'Check recordings'}</button>
        <button type="button" onClick={() => setSel(sel.size === files.length ? new Set() : new Set(files.map((f) => f.path)))}>{sel.size === files.length && files.length > 0 ? 'Select none' : 'Select all'}</button>
        {dark.length > 0 && <button type="button" onClick={() => setSel(new Set(dark.map((f) => f.path)))}>Select dark ({dark.length})</button>}
        <label className="fm-sort">Show <select value={show} onChange={(e) => setShow(e.target.value as typeof show)}>
          <option value="all">Everything</option><option value="rec">Recordings</option><option value="hdr">HDR lanes</option><option value="prominence">Prominence video</option>
        </select></label>
        <label className="fm-sort">Sort <select value={sort} onChange={(e) => setSort(e.target.value as typeof sort)}>
          <option value="new">Newest first</option><option value="big">Largest first</option><option value="name">Name</option>
        </select></label>
      </div>

      <div className="fm-list">
        {files.length === 0 && <div className="empty">Nothing captured yet. Record something in the Record tab and it appears here.</div>}
        {groups.map(([g, items]) => (
          <section key={g}>
            <h3 className="fm-day">{g} <span className="muted">· {items.length} · {size(items.reduce((a, f) => a + f.size, 0))}</span></h3>
            <ul className="fm">
              {items.map((f) => {
                const st = stats[f.path]
                const s = st && !('error' in st) ? st : undefined
                return (
                  <li key={f.path} className={`fm-row ${sel.has(f.path) ? 'sel' : ''}`}>
                    <label className="fm-check"><input type="checkbox" checked={sel.has(f.path)} onChange={() => toggle(f.path)} aria-label={`Select ${f.path}`} /></label>
                    <div className="fm-main">
                      <div className="fm-head">
                        <span className="fm-name">{sort === 'big' ? f.path : base(f.path)}{f.folder_files != null ? '/' : ''}</span>
                        {f.kind && <span className="tag">{KIND_LABEL[f.kind]}</span>}
                        {s?.mostly_dark && <span className="tag warn" title={`${Math.round(s.dark_fraction * 100)}% of sampled frames are dark`}>Mostly dark</span>}
                        {s && !s.complete && <span className="tag warn" title="The file ends early: the recording was cut off">Incomplete</span>}
                      </div>
                      <div className="muted fm-sub">
                        {time(f.modified)}{f.modified ? ' · ' : ''}{f.folder_files != null ? `${f.folder_files} frames · ` : ''}{size(f.size)}
                        {s && <> · {s.width}×{s.height} · {s.depth}-bit · {s.frames_present}/{s.frames} frames{s.fps ? ` · ${s.fps.toFixed(1)} fps` : ''}{s.duration_s ? ` · ${s.duration_s.toFixed(1)} s` : ''} · brightness {Math.round(s.mean_avg * 100)}% (min {Math.round(s.mean_min * 100)}, max {Math.round(s.mean_max * 100)})</>}
                        {st && 'error' in st && <> · {st.error}</>}
                      </div>
                      {f.lanes && f.lanes.length > 0 && (
                        <details className="fm-lanes">
                          <summary>{f.lanes.length} lanes: {f.lanes.map((l) => `${l.name} ${l.files.length}`).join(' · ')}</summary>
                          {f.lanes.map((l) => (
                            <div key={l.name} className="fm-lane">
                              <b>{l.name}</b> <span className="muted">{l.files.length} recordings · {size(l.size)}</span>
                              <ul>{l.files.map((x) => (
                                <li key={x.path}>{x.path.split('/').pop()} <span className="muted">{size(x.size)}</span> <a href={`/files/${x.path}`} download>Download</a>{isSer(x.path) && <> <button type="button" className="small" onClick={() => check(x.path)}>Check</button></>}{stats[x.path] && ('error' in stats[x.path] ? <span className="muted"> {(stats[x.path] as { error: string }).error}</span> : <span className="muted"> {(stats[x.path] as Stats).frames_present}/{(stats[x.path] as Stats).frames} frames · brightness {Math.round((stats[x.path] as Stats).mean_avg * 100)}%</span>)}</li>
                              ))}</ul>
                            </div>
                          ))}
                        </details>
                      )}
                    </div>
                    <div className="fm-actions">
                      {isSer(f.path) && <button type="button" className="small" onClick={() => check(f.path)}>{st ? 'Re-check' : 'Check'}</button>}
                      {f.folder_files == null && <a className="btn small" href={`/files/${f.path}`} download>Download</a>}
                    </div>
                  </li>
                )
              })}
            </ul>
          </section>
        ))}
      </div>

      <footer className="fm-foot">
        <ErrorLine error={err} />
        {busy && <div className="hint-text">Deleting is disabled while recording or writing.</div>}
        {!ask ? (
          <button type="button" className="danger wide" disabled={sel.size === 0 || busy} onClick={() => setAsk(true)}>
            {sel.size === 0 ? 'Select captures to delete' : `Delete ${sel.size} selected (${size(selSize)})…`}
          </button>
        ) : (
          <div className="confirm" role="alertdialog" aria-label="Confirm delete">
            <p>Delete {sel.size} capture{sel.size === 1 ? '' : 's'} ({size(selSize)}) permanently?</p>
            <div className="btnrow tight">
              <button type="button" className="grow" onClick={() => setAsk(false)}>Keep</button>
              <button type="button" className="danger grow" onClick={del}>Delete</button>
            </div>
          </div>
        )}
      </footer>
    </div>
  )
}
