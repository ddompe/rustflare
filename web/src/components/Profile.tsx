import { useState } from 'react'
import { api, Status } from '../api'
import { PROFILES, profileOf } from '../profile'
import { useAction, ErrorLine } from './ui'
import { setupLine } from './OpticalSetups'

/** The reason a control is greyed, shown as a line under it (a phone has no hover). Pair it with `aria-describedby={id}` on the control. */
export function Why({ id, text, subject }: { id: string; text: string | null; /** what is off, said first: "Prominence view is off: ..." */ subject?: string }) {
  return text ? <div className="gate-why hint-text" id={id} role="note">{subject ? <><b>{subject}</b> is off: </> : null}{text}</div> : null
}

/** Step 1 of a session: what is being imaged. One compact row (the choice), one line of what it means, and the long sentence only on request. */
export function ProfilePicker({ status }: { status: Status | null }) {
  const { err, run } = useAction()
  const [what, setWhat] = useState(false)
  const p = profileOf(status)
  const cur = PROFILES.find((x) => x.id === p)
  return (
    <div className="profile-pick">
      <div className="profile-row">
        <select aria-label="What are you imaging?" value={p} onChange={(e) => run(() => api('PUT', '/profile', { id: e.target.value }))}>
          {PROFILES.map((x) => <option key={x.id} value={x.id}>{x.label}</option>)}
        </select>
        <button type="button" className="quiet" aria-expanded={what} aria-controls="profile-what" onClick={() => setWhat(!what)}>What is this?</button>
      </div>
      <p className="profile-short">{cur?.short}{setupLine(status) ? <> · <span aria-label="Optical setup in use">{setupLine(status)}</span></> : null}</p>
      {what && (
        <p className="hint-text" id="profile-what">{cur?.blurb} The words and the tools that apply follow this choice. Nothing is hidden: what does not apply is greyed, with the reason under it.</p>
      )}
      <ErrorLine error={err} />
    </div>
  )
}
