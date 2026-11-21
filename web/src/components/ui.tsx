import { ReactNode, useState } from 'react'

/** Runs an async action, reporting failures inline. */
export function useAction() {
  const [err, setErr] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const run = async (f: () => Promise<unknown>) => {
    setBusy(true)
    try {
      await f()
      setErr(null)
    } catch (e) {
      setErr((e as Error).message)
    } finally {
      setBusy(false)
    }
  }
  return { err, busy, run }
}

export function Panel({ title, children, error }: { title: string; children: ReactNode; error?: string | null }) {
  return (
    <div className="panel">
      <h2>{title}</h2>
      {children}
      {error && <div className="err">{error}</div>}
    </div>
  )
}

export function Row({ label, children }: { label?: string; children: ReactNode }) {
  return (
    <div className="row">
      {label && <label>{label}</label>}
      {children}
    </div>
  )
}
