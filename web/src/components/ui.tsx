import { ReactNode, useId, useState } from 'react'

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

/** A titled block inside a tab. Sections are separated by whitespace and a hairline, never nested in cards. */
export function Section({ title, children, error, aside }: { title?: string; children: ReactNode; error?: string | null; aside?: ReactNode }) {
  return (
    <section className="sec">
      {(title || aside) && (
        <div className="sec-head">
          {title && <h2>{title}</h2>}
          {aside}
        </div>
      )}
      {children}
      <ErrorLine error={error} />
    </section>
  )
}

export function ErrorLine({ error }: { error?: string | null }) {
  return error ? <div className="err" role="alert">{error}</div> : null
}

/** Label above its control, so a 390 px screen never squeezes either. */
export function Field({ label, children, hint }: { label: string; children: (id: string) => ReactNode; hint?: ReactNode }) {
  const id = useId()
  return (
    <div className="field">
      <label htmlFor={id}>{label}</label>
      {children(id)}
      {hint && <div className="hint-text">{hint}</div>}
    </div>
  )
}

/** On/off switch (native checkbox underneath, so keyboard and screen readers just work). */
export function Switch({ checked, onChange, label, hint, disabled }: { checked: boolean; onChange: (v: boolean) => void; label: string; hint?: ReactNode; disabled?: boolean }) {
  return (
    <label className={`switch ${disabled ? 'is-disabled' : ''}`}>
      <input type="checkbox" role="switch" checked={checked} disabled={disabled} onChange={(e) => onChange(e.target.checked)} />
      <span className="sw-text">
        <span className="sw-label">{label}</span>
        {hint && <span className="hint-text">{hint}</span>}
      </span>
      <span className="sw-track" aria-hidden="true"><span className="sw-knob" /></span>
    </label>
  )
}

/** A small set of mutually exclusive choices, shown all at once. */
export function Segmented<T extends string>({ value, options, onChange, label, disabled }: { value: T; options: { id: T; label: string }[]; onChange: (v: T) => void; label: string; disabled?: boolean }) {
  return (
    <div className="seg" role="radiogroup" aria-label={label}>
      {options.map((o) => (
        <button key={o.id} type="button" role="radio" aria-checked={value === o.id} disabled={disabled} className={value === o.id ? 'on' : ''} onClick={() => onChange(o.id)}>
          {o.label}
        </button>
      ))}
    </div>
  )
}

/** Native <details>: collapsed by default, with a one-line summary of the current setting. */
export function Disclosure({ title, summary, children, open, onToggle }: { title: string; summary?: ReactNode; children: ReactNode; open?: boolean; onToggle?: (o: boolean) => void }) {
  return (
    <details className="disc" open={open} onToggle={onToggle ? (e) => onToggle((e.currentTarget as HTMLDetailsElement).open) : undefined}>
      <summary>
        <span className="disc-title">{title}</span>
        {summary != null && <span className="disc-sum">{summary}</span>}
        <svg className="chev" viewBox="0 0 12 12" width="14" height="14" aria-hidden="true"><path d="M2 4l4 4 4-4" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" /></svg>
      </summary>
      <div className="disc-body">{children}</div>
    </details>
  )
}

export function Empty({ children }: { children: ReactNode }) {
  return <p className="empty">{children}</p>
}

export function RangeField({ label, value, min, max, step, show, onChange }: { label: string; value: number; min: number; max: number; step: number; show: string; onChange: (v: number) => void }) {
  const id = useId()
  return (
    <div className="ctl">
      <div className="ctl-head"><label htmlFor={id}>{label}</label><output htmlFor={id} className="ctl-val">{show}</output></div>
      <input id={id} type="range" min={min} max={max} step={step} value={value} onChange={(e) => onChange(parseFloat(e.target.value))} />
    </div>
  )
}

/** "about 14:32 (in 1 h 12 min)" for a time `seconds` from now. */
export function endsText(seconds: number): string {
  const when = new Date(Date.now() + seconds * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
  const m = Math.round(seconds / 60)
  const left = seconds < 90 ? `in ${Math.max(1, Math.round(seconds))} s` : m < 90 ? `in ${m} min` : `in ${Math.floor(m / 60)} h ${m % 60} min`
  return `about ${when} (${left})`
}

/** A drawn snowflake: the cooler is on. */
export function Snow({ title = 'Cooler on' }: { title?: string }) {
  return (
    <svg className="snow" viewBox="0 0 20 20" width="14" height="14" role="img" aria-label={title}>
      <title>{title}</title>
      <g fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round">
        <path d="M10 2v16M3.1 6l13.8 8M3.1 14L16.9 6" />
        <path d="M7.8 3.4L10 5.4l2.2-2M7.8 16.6L10 14.6l2.2 2M2.6 8.9l2.9.5-.6 2.9M17.4 8.9l-2.9.5.6 2.9M2.6 11.1l2.9-.5-.6-2.9M17.4 11.1l-2.9-.5.6-2.9" />
      </g>
    </svg>
  )
}
