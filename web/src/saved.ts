import { useEffect, useRef, useState } from 'react'
import { api, Status } from './api'

/** What the last capture left behind, remembered for a few minutes after it ends so "did it save?" always has an answer. */
export interface Saved { file: string; name: string; dir: string; size: number | null; early: string | null }

const SHOW_MS = 5 * 60_000

/** "2026-11-27/seq_051747Z.ser" -> the name and its folder. */
const parts = (file: string) => { const i = file.lastIndexOf('/'); return { name: file.slice(i + 1), dir: i < 0 ? '' : file.slice(0, i) } }

/**
 * Watches the capture state: when a recording or a snap ends, the file is named with its size (read from the file list) for five
 * minutes, on the Record tab and among the notes. `dismiss` hides it sooner.
 */
export function useSaved(status: Status | null): [Saved | null, () => void] {
  const [saved, setSaved] = useState<Saved | null>(null)
  const was = useRef<string>('idle')
  const state = status?.capture.state
  const last = status?.capture.last_file ?? null
  const early = status?.capture.stopped_early ?? null
  useEffect(() => {
    if (!state) return
    const before = was.current
    was.current = state
    if (state !== 'idle' || before === 'idle' || !last) return
    const { name, dir } = parts(last)
    setSaved({ file: last, name, dir, size: null, early })
    api<{ entries: { path: string; size: number }[] }>('GET', `/files/list?path=${encodeURIComponent(dir)}`)
      .then((l) => { const e = l.entries.find((x) => x.path === last); if (e) setSaved((s) => (s && s.file === last ? { ...s, size: e.size } : s)) })
      .catch(() => { /* the line is still true without a size */ })
  }, [state, last]) // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    if (!saved) return
    const t = window.setTimeout(() => setSaved(null), SHOW_MS)
    return () => window.clearTimeout(t)
  }, [saved?.file]) // eslint-disable-line react-hooks/exhaustive-deps
  // a new recording replaces the line
  useEffect(() => { if (state && state !== 'idle') setSaved(null) }, [state])
  return [saved, () => setSaved(null)]
}

export const sizeText = (b: number | null) => (b == null ? '' : b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : `${(b / 1e6).toFixed(1)} MB`)
