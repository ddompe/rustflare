import { useEffect, useState } from 'react'
import { api, Status } from '../api'
import { Panel } from './ui'

interface FileEntry { path: string; size: number; modified: number | null; folder_files: number | null }

const size = (b: number) => (b > 1e9 ? `${(b / 1e9).toFixed(2)} GB` : b > 1e6 ? `${(b / 1e6).toFixed(1)} MB` : `${Math.max(1, Math.round(b / 1e3))} kB`)

export function FilesPanel({ status }: { status: Status | null }) {
  const [files, setFiles] = useState<FileEntry[]>([])
  const last = status?.capture.last_file
  useEffect(() => { api<FileEntry[]>('GET', '/files').then(setFiles).catch(() => {}) }, [last])
  return (
    <Panel title="Captures">
      {files.length === 0 && <span className="muted">Nothing captured yet</span>}
      <ul className="files">
        {files.slice(0, 40).map((f) => (
          <li key={f.path}>
            {f.folder_files != null ? <span>{f.path}/</span> : <a href={`/files/${f.path}`} download>{f.path}</a>}{' '}
            <span className="muted">{f.folder_files != null ? `${f.folder_files} frames · ` : ''}{size(f.size)}</span>
          </li>
        ))}
      </ul>
    </Panel>
  )
}
