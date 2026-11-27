import { Status } from '../api'
import { useSetting } from '../settings'

export type SetupStepId = 'profile' | 'optics' | 'site' | 'clock'

/** Which of the four first-session steps are done. Profile is always done (one is always chosen); a mount that cannot report its clock counts as done. */
export function setupDone(status: Status | null): Record<SetupStepId, boolean> {
  const st = status?.mount?.site_time
  return {
    profile: true,
    optics: !!status && (status.optics?.focal_length_mm != null || !!status.optics?.setup?.active),
    site: !!status?.site,
    clock: !status?.mount || !st || (st.time_ok !== false && st.place_ok !== false),
  }
}

const STEPS: { id: SetupStepId; n: number; label: string }[] = [
  { id: 'profile', n: 1, label: 'Profile' },
  { id: 'optics', n: 2, label: 'Optical setup' },
  { id: 'site', n: 3, label: 'Site' },
  { id: 'clock', n: 4, label: 'Mount time' },
]

/**
 * "Set up: 1 profile, 2 optics, 3 site, 4 mount time" for the first session: the steps still to do, each a tap to where it is done.
 * Shown on Camera and Aim until all four are done; can be dismissed; never blocks anything.
 */
export function SetupStrip({ status, onStep }: { status: Status | null; onStep: (id: SetupStepId) => void }) {
  const [dismissed, setDismissed] = useSetting('ui.setup_dismissed', false)
  if (!status || dismissed) return null
  const done = setupDone(status)
  const todo = STEPS.filter((s) => !done[s.id])
  if (todo.length === 0) return null
  return (
    <nav className="setup-strip" aria-label="First session set up">
      <span className="ss-label">Set up {4 - todo.length} of 4</span>
      {todo.map((s) => (
        <button key={s.id} type="button" onClick={() => onStep(s.id)}><span className="ss-n" aria-hidden="true">{s.n}</span>{s.label}</button>
      ))}
      <button type="button" className="quiet ss-x" aria-label="Hide the set up steps" onClick={() => setDismissed(true)}>
        <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><path d="M3 3l10 10M13 3L3 13" stroke="currentColor" strokeWidth="2" strokeLinecap="round" /></svg>
      </button>
    </nav>
  )
}
