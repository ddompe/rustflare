import { ReactNode, RefObject, useEffect, useRef } from 'react'

/**
 * A real modal: role=dialog and aria-modal, labelled by its title, focus moves in and Tab stays inside, the page behind is inert,
 * Escape closes, and focus goes back to whatever opened it. A full-screen sheet on a phone, a centred panel on a wide screen.
 * Render it last inside `.app` so that "everything else" is the page behind.
 */
export function Modal({ title, onClose, children, className = '', canEscape, bare, initialFocus }: { title: string; onClose: () => void; /** asked on Escape: false keeps the dialog open (unsaved typing, a search field, a preview on top), the visible Close and Cancel buttons still work */ canEscape?: (e: KeyboardEvent) => boolean; /** the caller draws the whole dialog (its own header and layout) inside `className`; the title is then only its aria-label */ bare?: boolean; /** bare only: what takes focus first (default: the close button of the standard header) */ initialFocus?: RefObject<HTMLElement>; /** one `.modal-view` holding a `.modal-body` (scrolls) and an optional `.modal-foot` (stays in reach) */ children: ReactNode; className?: string }) {
  const closeRef = useRef<HTMLButtonElement>(null)
  const boxRef = useRef<HTMLDivElement>(null)
  const closeFn = useRef(onClose)
  closeFn.current = onClose
  const escFn = useRef(canEscape)
  escFn.current = canEscape
  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null
    const first = initialFocus ?? closeRef
    first.current?.focus()
    const esc = (e: KeyboardEvent) => { if (e.key === 'Escape') { e.preventDefault(); if (escFn.current && !escFn.current(e)) return; closeFn.current() } }
    const trap = (e: KeyboardEvent) => {
      if (e.key !== 'Tab' || !boxRef.current) return
      // a dialog on top of the dialog (the file preview) holds the focus until it closes
      const scope = boxRef.current.querySelector<HTMLElement>('[role="dialog"]') ?? boxRef.current
      const items = Array.from(scope.querySelectorAll<HTMLElement>('a[href], button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), summary' + (bare ? '' : ', [tabindex="0"]'))).filter((el) => el.getClientRects().length > 0)
      if (!items.length) return
      const first = items[0], last = items[items.length - 1]
      const at = document.activeElement
      if (!scope.contains(at)) { e.preventDefault(); first.focus() }
      else if (e.shiftKey && at === first) { e.preventDefault(); last.focus() }
      else if (!e.shiftKey && at === last) { e.preventDefault(); first.focus() }
    }
    document.addEventListener('keydown', esc)
    document.addEventListener('keydown', trap)
    const behind = Array.from(document.querySelectorAll<HTMLElement>('.app > *')).filter((el) => !el.contains(boxRef.current))
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
  if (bare) return <div ref={boxRef} className={className} role="dialog" aria-modal="true" aria-label={title}>{children}</div>
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
