import { ReactNode, useEffect, useId, useRef, useState } from 'react'

/** A small popover: open state, a ref for the box that holds button and sheet, closes on a tap outside and on Escape (focus returns to the button). */
export function usePopover() {
  const [open, setOpen] = useState(false)
  const box = useRef<HTMLDivElement>(null)
  const button = useRef<HTMLButtonElement>(null)
  useEffect(() => {
    if (!open) return
    const away = (e: Event) => { if (!box.current?.contains(e.target as Node)) setOpen(false) }
    const esc = (e: KeyboardEvent) => { if (e.key === 'Escape') { setOpen(false); button.current?.focus() } }
    document.addEventListener('pointerdown', away)
    document.addEventListener('keydown', esc)
    return () => { document.removeEventListener('pointerdown', away); document.removeEventListener('keydown', esc) }
  }, [open])
  return { open, setOpen, box, button }
}

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
export function Switch({ checked, onChange, label, hint, disabled, describedBy }: { checked: boolean; onChange: (v: boolean) => void; label: string; hint?: ReactNode; disabled?: boolean; /** id of the line that says why this is greyed */ describedBy?: string }) {
  return (
    <label className={`switch ${disabled ? 'is-disabled' : ''}`}>
      <input type="checkbox" role="switch" checked={checked} disabled={disabled} aria-describedby={disabled ? describedBy : undefined} onChange={(e) => onChange(e.target.checked)} />
      <span className="sw-text">
        <span className="sw-label">{label}</span>
        {hint && <span className="hint-text">{hint}</span>}
      </span>
      <span className="sw-track" aria-hidden="true"><span className="sw-knob" /></span>
    </label>
  )
}

/**
 * A small set of mutually exclusive choices, shown all at once. An option that does not apply is greyed (still readable) and its reason
 * is said once per group under it: "Disc edge and Full disc are off: ...". Every greyed option points at its line with aria-describedby.
 */
export function Segmented<T extends string>({ value, options, onChange, label, disabled }: { value: T; options: { id: T; label: string; /** Why this option does not apply now. Options with the same text share one line. */ off?: string | null }[]; onChange: (v: T) => void; label: string; disabled?: boolean }) {
  const uid = useId()
  const reasons: { text: string; labels: string[] }[] = []
  options.forEach((o) => {
    if (!o.off) return
    const r = reasons.find((x) => x.text === o.off)
    if (r) r.labels.push(o.label); else reasons.push({ text: o.off, labels: [o.label] })
  })
  const lineId = (text: string) => `${uid}-why-${reasons.findIndex((r) => r.text === text)}`
  return (
    <>
      <div className="seg" role="radiogroup" aria-label={label}>
        {options.map((o) => (
          <button key={o.id} type="button" role="radio" aria-checked={value === o.id} aria-describedby={o.off ? lineId(o.off) : undefined} disabled={disabled || !!o.off} className={value === o.id ? 'on' : ''} onClick={() => onChange(o.id)}>
            {o.label}
          </button>
        ))}
      </div>
      {reasons.map((r) => <div key={r.text} id={lineId(r.text)} className="gate-why hint-text" role="note"><b>{r.labels.join(' and ')}</b> {r.labels.length > 1 ? 'are' : 'is'} off: {r.text}</div>)}
    </>
  )
}

export type Tone = 'ok' | 'warn' | 'bad' | 'info' | 'off' | 'rec'
/** A state mark that differs in shape as well as colour: check in a circle (ok), triangle (warn), diamond (bad), ring (info, off), dot (recording). */
export function ToneIcon({ tone, size = 14 }: { tone: Tone; size?: number }) {
  return (
    <svg className={`tone-icon t-${tone}`} viewBox="0 0 16 16" width={size} height={size} aria-hidden="true" focusable="false">
      {tone === 'ok' && <><circle cx="8" cy="8" r="7" fill="currentColor" /><path d="M4.6 8.2l2.4 2.4 4.4-5" fill="none" stroke="var(--mark-ink, #0e0c0b)" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round" /></>}
      {tone === 'warn' && <><path d="M8 1.5L15 14H1z" fill="currentColor" strokeLinejoin="round" stroke="currentColor" strokeWidth="1.2" /><path d="M8 6v4M8 11.6v.4" stroke="var(--mark-ink, #0e0c0b)" strokeWidth="1.8" strokeLinecap="round" /></>}
      {tone === 'bad' && <><path d="M8 .8L15.2 8 8 15.2.8 8z" fill="currentColor" /><path d="M5.6 5.6l4.8 4.8M10.4 5.6l-4.8 4.8" stroke="var(--mark-ink, #0e0c0b)" strokeWidth="1.8" strokeLinecap="round" /></>}
      {tone === 'info' && <><circle cx="8" cy="8" r="6.2" fill="none" stroke="currentColor" strokeWidth="1.8" /><path d="M8 7.2v4M8 4.9v.2" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" /></>}
      {tone === 'off' && <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" strokeWidth="1.8" />}
      {tone === 'rec' && <circle cx="8" cy="8" r="6" fill="currentColor" />}
    </svg>
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
