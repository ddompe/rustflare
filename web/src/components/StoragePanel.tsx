import { useEffect, useState } from 'react'
import { api, Status, StorageInfo } from '../api'
import { Panel, Row, useAction } from './ui'

const gb = (b: number | null) => (b == null ? '?' : b >= 1e12 ? `${(b / 1e12).toFixed(2)} TB` : `${(b / 1e9).toFixed(b >= 1e10 ? 0 : 1)} GB`)

/** Where captures are written: pick a USB drive, see free space and recording time left. */
export function StoragePanel({ status }: { status: Status | null }) {
  const [data, setData] = useState<{ current: StorageInfo; candidates: StorageInfo[] } | null>(null)
  const [sel, setSel] = useState('')
  const { err, run } = useAction()
  const busy = status?.capture.state !== 'idle' && status?.capture.state !== undefined

  const load = () => api<{ current: StorageInfo; candidates: StorageInfo[] }>('GET', '/storage').then(setData).catch(() => {})
  useEffect(() => {
    load()
    const t = window.setInterval(load, 4000)
    return () => window.clearInterval(t)
  }, [])

  if (!data) return null
  const cur = data.current
  const cam = status?.camera
  const bytesPerSec = cam && cam.fps > 0 ? (cam.width * cam.height * (cam.bits / 8) * cam.fps) : 0
  const minutes = cur.free != null && bytesPerSec > 0 ? cur.free / bytesPerSec / 60 : null
  const limit = cur.max_file_bytes != null && bytesPerSec > 0 ? cur.max_file_bytes / bytesPerSec / 60 : null
  const used = cur.total && cur.free != null ? 1 - cur.free / cur.total : 0
  const others = data.candidates.filter((c) => c.path !== cur.path)

  return (
    <Panel title="Storage" error={err}>
      <div className="small"><b>{cur.label}</b> <span className="muted">{cur.path}</span></div>
      {cur.available ? (
        <>
          <progress max={1} value={used} />
          <div className="muted small">{gb(cur.free)} free of {gb(cur.total)}{cur.fstype ? ` · ${cur.fstype}` : ''}{minutes != null ? ` · ≈ ${minutes >= 120 ? `${(minutes / 60).toFixed(1)} h` : `${minutes.toFixed(0)} min`} of recording at ${cam!.fps.toFixed(0)} fps` : ''}</div>
        </>
      ) : (
        <div className="err">Not available: {cur.problem}</div>
      )}
      {cur.max_file_bytes != null && (
        <div className="warn small">
          This drive is FAT32: a single file can't exceed 4 GB{limit != null ? `, i.e. ≈ ${limit.toFixed(1)} min of recording at this frame rate` : ''}. Longer sequences stop at the limit. Reformat to exFAT or ext4 for long captures.
        </div>
      )}
      <hr />
      <Row label="Store on">
        <select value={sel} onChange={(e) => setSel(e.target.value)}>
          <option value="">{others.length ? 'choose a drive…' : 'no other drives found'}</option>
          {others.map((c) => (
            <option key={c.path} value={c.path} disabled={!c.available}>
              {c.label} · {c.fstype ?? '?'} · {gb(c.free)} free{c.problem ? ` · ${c.problem}` : ''}
            </option>
          ))}
        </select>
        <button disabled={!sel || busy} onClick={() => run(async () => { await api('PUT', '/storage', { path: sel }); setSel(''); await load() })}>Use</button>
      </Row>
      {busy && <div className="muted small">Can't change storage while capturing.</div>}
      <div className="muted small">Captures go in a <code>rustflare</code> folder on the chosen drive. The choice is remembered.</div>
    </Panel>
  )
}
