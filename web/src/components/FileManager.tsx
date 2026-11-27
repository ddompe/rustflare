import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { api, Status } from '../api'
import { ErrorLine } from './ui'
import { useStorage } from './Storage'

interface StackInfo { frames: number | null; kept: number | null; seconds: number | null; dark: boolean | null; flat: boolean | null }
interface Entry {
  name: string; path: string; dir: boolean; type: string; size: number; files: number; capped: boolean; modified: number | null
  stack?: { output: string; info: StackInfo }; from?: { source: string; info: StackInfo }
}
interface Listing { path: string; entries: Entry[] }
interface Stats {
  width: number; height: number; depth: number; frames: number; frames_present: number; complete: boolean
  fps: number | null; duration_s: number | null; mean_min: number; mean_avg: number; mean_max: number
  sampled: number; dark_fraction: number; mostly_dark: boolean
}
type StatsOrError = Stats | { error: string }

const TYPE_LABEL: Record<string, string> = {
  stacks: 'Stacked images', logs: 'Logs and debug', day: 'Day', hdr: 'HDR lanes', prominence: 'Prominence video', lane: 'Lane', folder: 'Folder',
  recording: 'SER', stack: 'Stack', fits: 'FITS', image: 'Picture', notes: 'Notes', file: 'File',
}
const size = (b: number) => (b > 1e12 ? `${(b / 1e12).toFixed(2)} TB` : b > 1e9 ? `${(b / 1e9).toFixed(2)} GB` : b > 1e6 ? `${(b / 1e6).toFixed(1)} MB` : b > 0 ? `${Math.max(1, Math.round(b / 1e3))} kB` : '0 kB')
/** The modified time in this device's own zone, with the zone named: the file names and the day folders are in UTC. */
const when = (s: number | null) => (s ? new Date(s * 1000).toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit', timeZoneName: 'short' }) : '')
/** "seq_051747Z.ser" is named in UTC (05:17:47 UTC); a day folder is a UTC date. The names are not changed, they are explained. */
const utcNote = (name: string, type: string): string => {
  const m = /_(\d\d)(\d\d)(\d\d)Z/.exec(name)
  if (m) return `named in UTC: ${m[1]}:${m[2]}:${m[3]} UTC`
  return type === 'day' || /^\d{4}-\d\d-\d\d$/.test(name) ? 'UTC date' : ''
}
const parentOf = (p: string) => p.split('/').slice(0, -1).join('/')
const plural = (n: number, w: string) => `${n} ${w}${n === 1 ? '' : 's'}`

function useListing(path: string, refreshKey: unknown, enabled = true) {
  const [data, setData] = useState<Listing | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const load = useCallback(() => {
    if (!enabled) { setLoading(false); return Promise.resolve() }
    setLoading(true)
    return api<Listing>('GET', `/files/list?path=${encodeURIComponent(path)}`)
      .then((d) => { setData(d); setError(null) })
      .catch((e) => { setData(null); setError(e instanceof Error ? e.message : String(e)) })
      .finally(() => setLoading(false))
  }, [path, enabled])
  useEffect(() => { load() }, [load, refreshKey])
  return { data, error, loading, load }
}

/** Record-tab entry point: a summary line and the button that opens the full-screen manager. */
export function FileManagerButton({ status, onOpen }: { status: Status | null; onOpen: () => void }) {
  const { data } = useListing('', status?.capture.last_file)
  const n = data?.entries.reduce((a, e) => a + e.files, 0) ?? 0
  const total = data?.entries.reduce((a, e) => a + e.size, 0) ?? 0
  return (
    <button type="button" className="wide" onClick={onOpen}>
      Files · {n ? `${plural(n, 'file')}, ${size(total)}` : 'nothing captured yet'}
    </button>
  )
}

/** Folder tree for wide screens: the drive's folders, opened on demand. */
function Tree({ cwd, go, refreshKey }: { cwd: string; go: (p: string) => void; refreshKey: unknown }) {
  return <nav className="fm-tree" aria-label="Folders"><TreeNode path="" name="Captures" cwd={cwd} go={go} depth={0} refreshKey={refreshKey} /></nav>
}
function TreeNode({ path, name, cwd, go, depth, refreshKey }: { path: string; name: string; cwd: string; go: (p: string) => void; depth: number; refreshKey: unknown }) {
  const onPath = cwd === path || cwd.startsWith(path ? path + '/' : '')
  const [open, setOpen] = useState(depth === 0)
  const expanded = open || onPath
  const { data } = useListing(path, refreshKey, expanded)
  const dirs = expanded && data && data.path === path ? data.entries.filter((e) => e.dir) : []
  // `stacks` first, `logs` last, days newest first
  dirs.sort((a, b) => rank(a) - rank(b) || (a.type === 'day' ? b.name.localeCompare(a.name) : a.name.localeCompare(b.name)))
  return (
    <div role="group" aria-label={name}>
      <div className={`fm-tnode ${cwd === path ? 'cur' : ''}`} style={{ paddingLeft: `${depth * 0.9}rem` }}>
        <button type="button" className="fm-tog" aria-label={`${expanded ? 'Collapse' : 'Expand'} ${name}`} aria-expanded={expanded} onClick={() => setOpen(!expanded)}><svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true" style={{ transform: expanded ? 'rotate(90deg)' : undefined }}><path d="M4 2l4.5 4L4 10" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" /></svg></button>
        <button type="button" className="fm-tname" aria-current={cwd === path ? 'page' : undefined} onClick={() => go(path)}>{name}</button>
      </div>
      {expanded && dirs.map((d) => <TreeNode key={d.path} path={d.path} name={d.name} cwd={cwd} go={go} depth={depth + 1} refreshKey={refreshKey} />)}
    </div>
  )
}
const rank = (e: Entry) => (e.type === 'stacks' ? 0 : e.type === 'logs' ? 2 : 1)

/** Full-screen file manager: a breadcrumb path and one folder at a time (a folder tree beside it on wide screens). */
export function FileManagerScreen({ status, onClose }: { status: Status | null; onClose: () => void }) {
  const busy = !!status && status.capture.state !== 'idle'
  const [cwd, setCwd] = useState(() => sessionStorage.getItem('rf-fm-cwd') ?? '')
  const refreshKey = `${status?.capture.last_file}|${busy}|${status?.after_burst?.stacked}`
  const { data, error, loading, load } = useListing(cwd, refreshKey)
  const { data: storage } = useStorage()
  const [stats, setStats] = useState<Record<string, StatsOrError>>({})
  const [sel, setSel] = useState<Set<string>>(new Set())
  const [sort, setSort] = useState<'name' | 'new' | 'big'>('name')
  const [q, setQ] = useState('')
  const [checking, setChecking] = useState(false)
  const [ask, setAsk] = useState(false)
  const [peek, setPeek] = useState<number | null>(null)
  const [limit, setLimit] = useState(120)
  const [view, setView] = useState<'grid' | 'list'>(() => (localStorage.getItem('rf-fm-view') === 'list' ? 'list' : 'grid'))
  const [rs, setRs] = useState<Entry | null>(null)
  const [rsMsg, setRsMsg] = useState<string | null>(null)
  const [err, setErr] = useState<string | null>(null)
  const [treeW, setTreeW] = useState(() => { const n = Number(localStorage.getItem('rf-fm-tree')); return n >= 150 && n <= 520 ? n : 240 })
  const setW = (n: number) => { const w = Math.max(150, Math.min(520, Math.round(n))); setTreeW(w); localStorage.setItem('rf-fm-tree', String(w)) }
  const drag = useRef<{ x: number; w: number } | null>(null)
  const closeRef = useRef<HTMLButtonElement>(null)
  const listRef = useRef<HTMLDivElement>(null)

  const go = (p: string) => { setLimit(120); setPeek(null); setCwd(p); setSel(new Set()); setAsk(false); setErr(null); setQ(''); sessionStorage.setItem('rf-fm-cwd', p); listRef.current?.scrollTo(0, 0) }

  // A modal: focus moves in, Tab stays inside (and the page behind is inert), Escape closes, and focus goes back to whatever opened it.
  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null
    closeRef.current?.focus()
    const esc = (e: KeyboardEvent) => { if (e.key === 'Escape' && !(e.target instanceof HTMLInputElement) && !document.querySelector('.fm-peek')) onClose() }
    const trap = (e: KeyboardEvent) => {
      if (e.key !== 'Tab') return
      const scope = document.querySelector('.fm-peek') ?? document.querySelector('.fm-screen')
      if (!scope) return
      const items = Array.from(scope.querySelectorAll<HTMLElement>('a[href], button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), summary')).filter((el) => el.getClientRects().length > 0)
      if (!items.length) return
      const first = items[0], last = items[items.length - 1]
      const at = document.activeElement
      if (!scope.contains(at)) { e.preventDefault(); first.focus() }
      else if (e.shiftKey && at === first) { e.preventDefault(); last.focus() }
      else if (!e.shiftKey && at === last) { e.preventDefault(); first.focus() }
    }
    document.addEventListener('keydown', esc)
    document.addEventListener('keydown', trap)
    const behind = Array.from(document.querySelectorAll<HTMLElement>('.app > :not(.fm-screen)'))
    behind.forEach((el) => el.setAttribute('inert', ''))
    const prev = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    return () => {
      document.removeEventListener('keydown', esc)
      document.removeEventListener('keydown', trap)
      behind.forEach((el) => el.removeAttribute('inert'))
      document.body.style.overflow = prev
      opener?.focus()
    }
  }, [onClose])

  // a remembered folder that no longer exists (deleted, other drive): say so, and go back to the top
  useEffect(() => { if (error && cwd) { setErr(`${cwd} is not on the drive any more: showing the top.`); setCwd(''); sessionStorage.removeItem('rf-fm-cwd') } }, [error, cwd])

  const entries = data?.entries ?? []
  const shown = useMemo(() => {
    const f = entries.filter((e) => !q || e.name.toLowerCase().includes(q.toLowerCase()))
    const dates = entries.filter((e) => e.dir).length > 0 && entries.filter((e) => e.dir).every((e) => /^\d{4}-\d\d-\d\d$/.test(e.name))
    const by = (a: Entry, b: Entry) => (sort === 'big' ? b.size - a.size : sort === 'new' ? (b.modified ?? 0) - (a.modified ?? 0) : cwd === '' || (dates && a.dir && b.dir) ? b.name.localeCompare(a.name) : a.name.localeCompare(b.name, undefined, { numeric: true }))
    return f.sort((a, b) => (cwd === '' ? rank(a) - rank(b) : Number(b.dir) - Number(a.dir)) || by(a, b))
  }, [entries, q, sort, cwd])

  const stacksShown = useMemo(() => shown.filter((e) => e.type === 'stack'), [shown])
  const grid = view === 'grid' && stacksShown.length > 0
  const visible = shown.slice(0, limit)
  useEffect(() => { setLimit(120) }, [q, sort])
  const check = async (p: string) => {
    try { const s = await api<Stats>('GET', `/files/stats?path=${encodeURIComponent(p)}`); setStats((x) => ({ ...x, [p]: s })) }
    catch (e) { setStats((x) => ({ ...x, [p]: { error: e instanceof Error ? e.message : String(e) } })) }
  }
  const recs = shown.filter((e) => e.type === 'recording')
  const checkAll = async () => { setChecking(true); for (const f of recs) await check(f.path); setChecking(false) }
  const toggle = (p: string) => setSel((s) => { const n = new Set(s); if (n.has(p)) n.delete(p); else n.add(p); return n })
  const okStat = (p: string) => { const s = stats[p]; return s && !('error' in s) ? s : undefined }
  const dark = entries.filter((f) => okStat(f.path)?.mostly_dark)
  const cut = entries.filter((f) => okStat(f.path) && !okStat(f.path)!.complete)
  const selectable = shown.filter((e) => cwd !== '' || !['stacks', 'logs', 'day'].includes(e.type))
  const selEntries = entries.filter((e) => sel.has(e.path))
  const selSize = selEntries.reduce((a, f) => a + f.size, 0)
  const selFiles = selEntries.reduce((a, f) => a + f.files, 0)
  const total = entries.reduce((a, f) => a + f.size, 0)
  const free = storage?.current.free
  const crumbs = cwd ? cwd.split('/') : []
  const crumbsRef = useRef<HTMLElement>(null)
  useEffect(() => { crumbsRef.current?.scrollTo({ left: 9999 }) }, [cwd])

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

  const row = (f: Entry) => {
    const st = stats[f.path]
    const s = okStat(f.path)
    const info = f.from?.info ?? f.stack?.info
    const protectedTop = cwd === '' && ['stacks', 'logs', 'day'].includes(f.type)
    return (
      <li key={f.path} className={`fm-row ${sel.has(f.path) ? 'sel' : ''} ${protectedTop ? 'nocheck' : ''} ${f.type === 'stacks' ? 'hero' : ''}`} data-type={f.type}>
        {protectedTop ? null : <label className="fm-check"><input type="checkbox" checked={sel.has(f.path)} onChange={() => toggle(f.path)} aria-label={`Select ${f.name}`} /></label>}
        <div className="fm-main">
          <div className="fm-head">
            {f.type === 'stack' && <img className="fm-thumb" loading="lazy" alt="" src={`/api/stack/thumb/${f.path.replace(/^stacks\//, '')}`} />}
            {f.dir
              ? <button type="button" className="fm-open" onClick={() => go(f.path)}><svg className="fm-ico" viewBox="0 0 12 12" width="12" height="12" aria-hidden="true"><path d="M4 2l4.5 4L4 10" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" /></svg>{f.type === 'stacks' ? 'Stacked images' : f.name}</button>
              : <span className="fm-name">{f.name}</span>}
            {!['stack', 'recording', 'notes', 'file', 'image', 'fits', 'folder', 'day'].includes(f.type) && <span className="tag">{TYPE_LABEL[f.type] ?? f.type}</span>}
            {f.stack && <button type="button" className="tag ok linkish" onClick={() => go(parentOf(f.stack!.output))}>Stacked, show</button>}
            {s?.mostly_dark && <span className="tag warn" title={`${Math.round(s.dark_fraction * 100)}% of sampled frames are dark`}>Mostly dark</span>}
            {s && !s.complete && <span className="tag warn" title="The file ends early: the recording was cut off">Incomplete</span>}
          </div>
          <div className="muted fm-sub">
            {f.type === 'stacks' && <>Your finished stacks, by day · </>}{when(f.modified)}{f.modified ? ' · ' : ''}{f.dir ? `${plural(f.files, 'file')} · ` : ''}{f.capped ? 'at least ' : ''}{size(f.size)}{utcNote(f.name, f.type) ? ` · ${utcNote(f.name, f.type)}` : ''}
            {f.type === 'stack' && info && <> · best {info.kept} of {info.frames} frames{info.seconds ? ` · stacked in ${info.seconds.toFixed(0)} s` : ''}{info.dark || info.flat ? ` · ${[info.dark && 'dark', info.flat && 'flat'].filter(Boolean).join(' + ')} applied` : ' · no dark or flat'}</>}
            {s && <> · {s.width}×{s.height} · {s.depth}-bit · {s.frames_present}/{s.frames} frames{s.fps ? ` · ${s.fps.toFixed(1)} fps` : ''}{s.duration_s ? ` · ${s.duration_s.toFixed(1)} s` : ''} · brightness {Math.round(s.mean_avg * 100)}% (min {Math.round(s.mean_min * 100)}, max {Math.round(s.mean_max * 100)})</>}
            {st && 'error' in st && <> · {st.error}</>}
          </div>
          {f.from && <div className="muted fm-sub">From <button type="button" className="linklike" onClick={() => go(parentOf(f.from!.source))}>{f.from.source.split('/').pop()}</button></div>}
        </div>
        <div className="fm-actions">
          {f.path.startsWith('stacks/') && f.files > 0 && !busy && <button type="button" className="small" onClick={() => { setRs(f); setErr(null) }}>Re-stack</button>}
          {f.type === 'recording' && <button type="button" className="small" onClick={() => check(f.path)}>{st ? 'Re-check' : 'Check'}</button>}
          {f.dir
            ? (f.files === 0 ? <span className="muted">Empty</span> : <a className="btn small" href={`/api/files/archive?path=${encodeURIComponent(f.path)}`} download aria-label={`Download ${f.name} as one .tar file, ${size(f.size)}`}>Download all · {size(f.size)}</a>)
            : <a className="btn small" href={`/files/${f.path}`} download>Download</a>}
        </div>
      </li>
    )
  }

  return (
    <div className="fm-screen" role="dialog" aria-modal="true" aria-label="Files">
      <header className="fm-top">
        <h2>Files</h2>
        <div className="fm-summary muted">
          {loading && !data ? 'Loading…' : <>{plural(entries.length, 'item')} · {size(total)}{free != null ? ` · ${size(free)} free on the drive` : ''}</>}
          {dark.length > 0 && <> · <b className="warn-text">{dark.length} mostly dark</b></>}
          {cut.length > 0 && <> · <b className="warn-text">{cut.length} incomplete</b></>}
        </div>
        <button ref={closeRef} type="button" className="fm-close" onClick={onClose} aria-label="Close Files"><svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true"><path d="M3 3l10 10M13 3L3 13" stroke="currentColor" strokeWidth="2" strokeLinecap="round" /></svg></button>
      </header>

      <nav className="fm-crumbs" aria-label="Folder path" ref={crumbsRef}>
        {cwd && <button type="button" className="fm-up" onClick={() => go(parentOf(cwd))} aria-label="Up one folder"><svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true"><path d="M8 13V3M3.5 7.5L8 3l4.5 4.5" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" /></svg></button>}
        <button type="button" className="fm-crumb" aria-current={cwd === '' ? 'page' : undefined} onClick={() => go('')}>Captures</button>
        {crumbs.map((c, i) => {
          const p = crumbs.slice(0, i + 1).join('/')
          return <span key={p} className="fm-crumbwrap"><span aria-hidden="true">›</span><button type="button" className="fm-crumb" aria-current={i === crumbs.length - 1 ? 'page' : undefined} onClick={() => go(p)}>{c}</button></span>
        })}
      </nav>

      <div className="fm-bar">
        <input type="search" className="fm-find" placeholder="Filter this folder" aria-label="Filter this folder by name" value={q} onChange={(e) => setQ(e.target.value)} />
        <label className="fm-sort">Sort <select value={sort} onChange={(e) => setSort(e.target.value as typeof sort)}>
          <option value="name">Name</option><option value="new">Newest first</option><option value="big">Largest first</option>
        </select></label>
        {selectable.length > 0 && <button type="button" onClick={() => setSel(sel.size === selectable.length ? new Set() : new Set(selectable.map((f) => f.path)))}>{sel.size === selectable.length && sel.size > 0 ? 'Select none' : 'Select all'}</button>}
        {recs.length > 0 && <button type="button" disabled={checking || busy} onClick={checkAll}>{checking ? 'Checking…' : `Check ${plural(recs.length, 'recording')}`}</button>}
        {total > 1e9 && cwd !== '' && <div className="hint-text fm-big">{size(total)} is a long download over Wi-Fi: keep this tab open until it finishes. The file is a .tar, which opens by double-click on a Mac or PC.</div>}
        {dark.length > 0 && <button type="button" onClick={() => setSel(new Set(dark.map((f) => f.path)))}>Select dark ({dark.length})</button>}
        {cwd !== '' && total > 0 && <a className="btn" href={`/api/files/archive?path=${encodeURIComponent(cwd)}`} download>Download this folder · {size(total)}</a>}
      </div>

      <div className="fm-body" style={{ ['--fm-tree-w' as string]: `${treeW}px` }}>
        <Tree cwd={cwd} go={go} refreshKey={refreshKey} />
        <div
          className="fm-divider" role="separator" aria-orientation="vertical" aria-label="Resize the folder tree" aria-valuemin={150} aria-valuemax={520} aria-valuenow={treeW} tabIndex={0}
          onPointerDown={(e) => { drag.current = { x: e.clientX, w: treeW }; e.currentTarget.setPointerCapture(e.pointerId) }}
          onPointerMove={(e) => { if (drag.current) setW(drag.current.w + e.clientX - drag.current.x) }}
          onPointerUp={() => { drag.current = null }}
          onPointerCancel={() => { drag.current = null }}
          onDoubleClick={() => setW(240)}
          onKeyDown={(e) => { if (e.key === 'ArrowLeft') { e.preventDefault(); setW(treeW - 20) } else if (e.key === 'ArrowRight') { e.preventDefault(); setW(treeW + 20) } else if (e.key === 'Home') { e.preventDefault(); setW(240) } }}
        />
        <div className="fm-list" ref={listRef}>
          {cwd === '' && entries.length === 0 && !loading && <div className="empty">Nothing captured yet. Record something in the Record tab and it appears here.</div>}
          {cwd !== '' && entries.length === 0 && !loading && !error && <div className="empty">This folder is empty.</div>}
          {q && shown.length === 0 && entries.length > 0 && <div className="empty">Nothing in this folder matches “{q}”.</div>}
          {stacksShown.length > 0 && (
            <div className="fm-viewbar" role="group" aria-label="Show the stacks as">
              <button type="button" aria-pressed={grid} onClick={() => { setView('grid'); localStorage.setItem('rf-fm-view', 'grid') }}>Thumbnails</button>
              <button type="button" aria-pressed={!grid} onClick={() => { setView('list'); localStorage.setItem('rf-fm-view', 'list') }}>List</button>
            </div>
          )}
          <ul className="fm">{(grid ? visible.filter((e) => e.type !== 'stack') : visible).map(row)}</ul>
          {grid && (
            <ul className="fm-grid" aria-label="Stacks">
              {visible.filter((e) => e.type === 'stack').map((f) => (
                <li key={f.path} className={sel.has(f.path) ? 'sel' : ''}>
                  <button type="button" className="fm-cell" aria-label={`Preview ${f.name}`} onClick={() => setPeek(stacksShown.findIndex((x) => x.path === f.path))}>
                    <img loading="lazy" alt="" src={`/api/stack/thumb/${f.path.replace(/^stacks\//, '')}`} />
                    <span className="fm-cellname">{f.name.replace(/^stack_/, '').replace(/\.fits$/, '')}</span>
                  </button>
                  <label className="fm-cellcheck"><input type="checkbox" checked={sel.has(f.path)} onChange={() => toggle(f.path)} aria-label={`Select ${f.name}`} /></label>
                </li>
              ))}
            </ul>
          )}
          {shown.length > limit && <button type="button" className="wide" onClick={() => setLimit(limit + 120)}>Show {Math.min(120, shown.length - limit)} more ({shown.length - limit} not shown)</button>}
        </div>
      </div>

      {peek != null && stacksShown[peek] && <Peek items={stacksShown} index={peek} setIndex={setPeek} />}

      <footer className="fm-foot">
        <ErrorLine error={err} />
        {rsMsg && <div className="hint-text" role="status">{rsMsg}</div>}
        {rs && (
          <div className="confirm" role="alertdialog" aria-label="Confirm re-stack">
            <p>Delete {rs.dir ? `the ${plural(rs.files, 'stack')} in ${rs.name}` : rs.name} and make {rs.dir || rs.files > 1 ? 'them' : 'it'} again from the recording with the current stacker settings?</p>
            <div className="btnrow tight">
              <button type="button" className="grow" onClick={() => setRs(null)}>Keep</button>
              <button type="button" className="danger grow" onClick={async () => {
                try { const r = await api<{ queued: number }>('POST', '/stack/restack', { path: rs.path, confirm: true }); setRsMsg(`${plural(r.queued, 'stack')} will be made again by the stacker.`); load() }
                catch (e) { setErr(e instanceof Error ? e.message : String(e)) }
                setRs(null)
              }}>Re-stack</button>
            </div>
          </div>
        )}
        {busy && <div className="hint-text">Deleting is disabled while recording or writing.</div>}
        {sel.size > 0 && !ask && <a className="btn wide" href={`/api/files/archive?path=${encodeURIComponent(cwd)}&names=${encodeURIComponent(JSON.stringify(selEntries.map((e) => e.name)))}`} download>Download {plural(sel.size, 'selected item')} · {size(selSize)} (.tar)</a>}
        {!ask ? (selectable.length === 0 ? null : <button type="button" className="danger wide" disabled={sel.size === 0 || busy} onClick={() => setAsk(true)}>
            {sel.size === 0 ? 'Select items to delete' : `Delete ${plural(sel.size, 'item')} (${size(selSize)})…`}
          </button>
        ) : (
          <div className="confirm" role="alertdialog" aria-label="Confirm delete">
            <p className="fm-names">{selEntries.slice(0, 3).map((e) => e.name).join(', ')}{selEntries.length > 3 ? ` and ${selEntries.length - 3} more` : ''}</p>
            {selEntries.some((e) => e.type === 'recording' && !e.stack) && <p className="warn">{plural(selEntries.filter((e) => e.type === 'recording' && !e.stack).length, 'recording')} {selEntries.filter((e) => e.type === 'recording' && !e.stack).length === 1 ? 'has' : 'have'} not been stacked: the raw data is gone for good.</p>}
            <p>Delete {plural(sel.size, 'item')}{selFiles > sel.size ? `, ${plural(selFiles, 'file')} in all,` : ''} ({size(selSize)}) permanently?{free != null ? ` That frees ${size(selSize)}: ${size(free + selSize)} will be free.` : ''}</p>
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

/** Preview of a stack (a picture made for display, not the data) with the previous and next one in the folder. */
function Peek({ items, index, setIndex }: { items: Entry[]; index: number; setIndex: (i: number | null) => void }) {
  const f = items[index]
  const info = f.from?.info
  const ref = useRef<HTMLButtonElement>(null)
  useEffect(() => { ref.current?.focus() }, [])
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === 'Escape') { e.stopPropagation(); setIndex(null) }
      else if (e.key === 'ArrowLeft') setIndex(Math.max(0, index - 1))
      else if (e.key === 'ArrowRight') setIndex(Math.min(items.length - 1, index + 1))
    }
    document.addEventListener('keydown', key, true)
    return () => document.removeEventListener('keydown', key, true)
  }, [index, items.length, setIndex])
  return (
    <div className="fm-peek" role="dialog" aria-modal="true" aria-label={`Preview of ${f.name}`}>
      <header>
        <b>{f.name}</b>
        <span className="muted">{index + 1} of {items.length}{info ? ` · best ${info.kept} of ${info.frames} frames` : ''}</span>
        <button ref={ref} type="button" onClick={() => setIndex(null)} aria-label="Close preview"><svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true"><path d="M3 3l10 10M13 3L3 13" stroke="currentColor" strokeWidth="2" strokeLinecap="round" /></svg></button>
      </header>
      <img alt={`Preview of ${f.name}, stretched for display`} src={`/api/stack/thumb/${f.path.replace(/^stacks\//, '')}`} />
      <footer>
        <button type="button" disabled={index === 0} onClick={() => setIndex(index - 1)}>‹ Previous</button>
        <a className="btn" href={`/files/${f.path}`} download>Download · {size(f.size)}</a>
        <button type="button" disabled={index === items.length - 1} onClick={() => setIndex(index + 1)}>Next ›</button>
      </footer>
      <p className="muted fm-peekhint">A preview stretched for display; the FITS file has the data.</p>
    </div>
  )
}
