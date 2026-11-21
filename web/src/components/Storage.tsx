import { useEffect, useState } from 'react'
import { api, Status, StorageInfo } from '../api'
import { ErrorLine, useAction } from './ui'

const gb = (b: number | null) => (b == null ? '?' : b >= 1e12 ? `${(b / 1e12).toFixed(2)} TB` : `${(b / 1e9).toFixed(b >= 1e10 ? 0 : 1)} GB`)

export interface StorageData { current: StorageInfo; candidates: StorageInfo[] }

/** Current storage location and candidate drives, refreshed every few seconds while mounted. */
export function useStorage() {
  const [data, setData] = useState<StorageData | null>(null)
  const load = () => api<StorageData>('GET', '/storage').then(setData).catch(() => {})
  useEffect(() => {
    load()
    const t = window.setInterval(load, 4000)
    return () => window.clearInterval(t)
  }, [])
  return { data, load }
}

function recordingMinutes(cur: StorageInfo, status: Status | null) {
  const cam = status?.camera
  const bytesPerSec = cam && cam.fps > 0 ? cam.width * cam.height * (cam.bits / 8) * cam.fps : 0
  return {
    bytesPerSec,
    minutes: cur.free != null && bytesPerSec > 0 ? cur.free / bytesPerSec / 60 : null,
    limit: cur.max_file_bytes != null && bytesPerSec > 0 ? cur.max_file_bytes / bytesPerSec / 60 : null,
  }
}

function Fat32Warning({ cur, limit }: { cur: StorageInfo; limit: number | null }) {
  if (cur.max_file_bytes == null) return null
  return (
    <div className="warn" role="alert">
      This drive is FAT32: a single file can't exceed 4 GB{limit != null ? `, i.e. ≈ ${limit.toFixed(1)} min of recording at this frame rate` : ''}. Longer sequences stop at the limit. Reformat to exFAT or ext4 for long captures.
    </div>
  )
}

/** One read-only line on the Record tab: where it is saving, how much room is left, and any warning. */
export function StorageLine({ status, onChange }: { status: Status | null; onChange: () => void }) {
  const { data } = useStorage()
  if (!data) return null
  const cur = data.current
  const { minutes, limit } = recordingMinutes(cur, status)
  const cam = status?.camera
  return (
    <div className="store-line">
      <div className="store-main">
        <span><b>Saving to {cur.label}</b>{' '}
          {cur.available
            ? <span className="muted">{gb(cur.free)} free{minutes != null ? ` · ≈ ${minutes >= 120 ? `${(minutes / 60).toFixed(1)} h` : `${minutes.toFixed(0)} min`} at ${cam!.fps.toFixed(0)} fps` : ''}</span>
            : <span className="bad-text">not available: {cur.problem}</span>}
        </span>
        <button type="button" className="quiet small" onClick={onChange}>Change</button>
      </div>
      <Fat32Warning cur={cur} limit={limit} />
    </div>
  )
}

/** Where captures are written: pick a USB drive, see free space and recording time left. */
export function StorageSettings({ status }: { status: Status | null }) {
  const { data, load } = useStorage()
  const [sel, setSel] = useState('')
  const { err, run } = useAction()
  const [ejected, setEjected] = useState<string | null>(null)
  const busy = status?.capture.state !== 'idle' && status?.capture.state !== undefined
  if (!data) return <div className="muted">Loading…</div>
  const cur = data.current
  const cam = status?.camera
  const { minutes, limit } = recordingMinutes(cur, status)
  const used = cur.total && cur.free != null ? 1 - cur.free / cur.total : 0
  const others = data.candidates.filter((c) => c.path !== cur.path)
  return (
    <div>
      <div><b>{cur.label}</b> <span className="muted">{cur.path}</span></div>
      {cur.available ? (
        <>
          <progress max={1} value={used} aria-label="Disk used" />
          <div className="hint-text">{gb(cur.free)} free of {gb(cur.total)}{cur.fstype ? ` · ${cur.fstype}` : ''}{minutes != null ? ` · ≈ ${minutes >= 120 ? `${(minutes / 60).toFixed(1)} h` : `${minutes.toFixed(0)} min`} of recording at ${cam!.fps.toFixed(0)} fps` : ''}</div>
        </>
      ) : (
        <div className="err">Not available: {cur.problem}</div>
      )}
      <Fat32Warning cur={cur} limit={limit} />
      <div className="field">
        <label htmlFor="store-sel">Store on</label>
        <select id="store-sel" value={sel} onChange={(e) => setSel(e.target.value)}>
          <option value="">{others.length ? 'choose a drive…' : 'no other drives found'}</option>
          {others.map((c) => (
            <option key={c.path} value={c.path} disabled={!c.available}>
              {c.label} · {c.fstype ?? '?'} · {gb(c.free)} free{c.problem ? ` · ${c.problem}` : ''}
            </option>
          ))}
        </select>
      </div>
      <button type="button" className="wide" disabled={!sel || busy} onClick={() => run(async () => { await api('PUT', '/storage', { path: sel }); setSel(''); await load() })}>Use this drive</button>
      {cur.mount_point && data.candidates.some((c) => c.mount_point === cur.mount_point) && (
        <button type="button" className="wide" disabled={busy} onClick={() => run(async () => { const r = await api<{ message: string }>('POST', '/storage/eject', { mount_point: cur.mount_point }); setEjected(r.message); await load() })}>Eject drive safely</button>
      )}
      {ejected && <div className="hint-text" role="status">{ejected}</div>}
      {busy && <div className="hint-text">Can't change or eject storage while capturing or writing.</div>}
      <div className="hint-text">Captures go in a <code>rustflare</code> folder on the chosen drive. The choice is remembered.</div>
      <ErrorLine error={err} />
    </div>
  )
}
