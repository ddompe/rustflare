import { ReactNode, useEffect, useRef } from 'react'

/**
 * A real modal: role=dialog and aria-modal, labelled by its title, focus moves in and Tab stays inside, the page behind is inert,
 * Escape closes, and focus goes back to whatever opened it. A full-screen sheet on a phone, a centred panel on a wide screen.
 * Render it last inside `.app` so that "everything else" is the page behind.
 */
export function Modal({ title, onClose, children, className = '' }: { title: string; onClose: () => void; /** one `.modal-view` holding a `.modal-body` (scrolls) and an optional `.modal-foot` (stays in reach) */ children: ReactNode; className?: string }) {
  const closeRef = useRef<HTMLButtonElement>(null)
  const boxRef = useRef<HTMLDivElement>(null)
  const closeFn = useRef(onClose)
  closeFn.current = onClose
  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null
    closeRef.current?.focus()
    const esc = (e: KeyboardEvent) => { if (e.key === 'Escape') { e.preventDefault(); closeFn.current() } }
    const trap = (e: KeyboardEvent) => {
      if (e.key !== 'Tab' || !boxRef.current) return
      const items = Array.from(boxRef.current.querySelectorAll<HTMLElement>('a[href], button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex="0"]')).filter((el) => el.getClientRects().length > 0)
      if (!items.length) return
      const first = items[0], last = items[items.length - 1]
      const at = document.activeElement
      if (!boxRef.current.contains(at)) { e.preventDefault(); first.focus() }
      else if (e.shiftKey && at === first) { e.preventDefault(); last.focus() }
      else if (!e.shiftKey && at === last) { e.preventDefault(); first.focus() }
    }
    document.addEventListener('keydown', esc)
    document.addEventListener('keydown', trap)
    const behind = Array.from(document.querySelectorAll<HTMLElement>('.app > :not(.modal-back)'))
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
  }, [])
  return (
    <div className="modal-back">
      <div ref={boxRef} className={`modal-panel ${className}`} role="dialog" aria-modal="true" aria-labelledby="modal-title">
        <header className="modal-head">
          <h2 id="modal-title">{title}</h2>
          <button ref={closeRef} type="button" className="modal-close" onClick={onClose} aria-label={`Close ${title}`}><svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true"><path d="M3 3l10 10M13 3L3 13" stroke="currentColor" strokeWidth="2" strokeLinecap="round" /></svg></button>
        </header>
        {children}
      </div>
    </div>
  )
}
