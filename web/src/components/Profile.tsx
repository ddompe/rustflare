import { api, Status } from '../api'
import { PROFILES, profileOf } from '../profile'
import { Segmented, useAction, ErrorLine } from './ui'
import { setupLine } from './OpticalSetups'

/** The reason a control is greyed, shown as a line under it (a phone has no hover). Pair it with `aria-describedby={id}` on the control. */
export function Why({ id, text }: { id: string; text: string | null }) {
  return text ? <div className="gate-why hint-text" id={id} role="note">{text}</div> : null
}

/** Step 1 of a session: what is being imaged. */
export function ProfilePicker({ status }: { status: Status | null }) {
  const { err, run } = useAction()
  const p = profileOf(status)
  return (
    <div className="profile-pick">
      <Segmented label="What are you imaging?" value={p} onChange={(id) => run(() => api('PUT', '/profile', { id }))} options={PROFILES.map((x) => ({ id: x.id, label: x.label }))} />
      {setupLine(status) && <p className="setup-line" aria-label="Optical setup in use">{setupLine(status)}</p>}
      <p className="hint-text">{PROFILES.find((x) => x.id === p)?.blurb} The words and the tools that apply follow this choice; nothing is hidden, and what does not apply is greyed with the reason.</p>
      <ErrorLine error={err} />
    </div>
  )
}
