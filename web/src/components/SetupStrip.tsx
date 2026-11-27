import { Status } from '../api'
import { useSetting } from '../settings'
import { ToneIcon, usePopover } from './ui'
import { profileOf, PROFILES } from '../profile'

export type SetupStepId = 'camera' | 'profile' | 'optics' | 'site' | 'mount'

/**
 * Which first-session steps are really done. Only confirmed things count: a camera that is open, a profile the user chose or confirmed
 * (never done by default), an optical setup applied or optics entered, a site that is saved, a mount that is connected with a matching
 * clock (one that was sent by itself counts once it matches). With no mount, step 5 is not done: it says "Connect the mount".
 */
export function setupDone(status: Status | null, profileConfirmed: boolean): Record<SetupStepId, boolean> {
  const m = status?.mount
  const st = m?.site_time
  return {
    camera: !!status?.camera,
    profile: profileConfirmed,
    optics: !!status && (status.optics?.focal_length_mm != null || !!status.optics?.setup?.active),
    site: !!status?.site,
    mount: !!m && (!st || (st.time_ok !== false && st.place_ok !== false)),
  }
}

const STEPS: SetupStepId[] = ['camera', 'profile', 'optics', 'site', 'mount']
const stepLabel = (id: SetupStepId, status: Status | null): string => ({
  camera: 'Open the camera', profile: 'Profile', optics: 'Optical setup', site: 'Site',
  mount: status?.mount ? 'Mount time' : 'Connect the mount',
})[id]

/** Where the first-session set-up stands: how many of the 5 steps are done and which are left, in order. Null when finished or dismissed. */
export function useSetup(status: Status | null) {
  const [dismissed, setDismissed] = useSetting('ui.setup_dismissed', false)
  const [confirmed, setConfirmed] = useSetting('ui.profile_confirmed', false)
  if (!status || dismissed) return null
  const done = setupDone(status, confirmed)
  const todo = STEPS.filter((s) => !done[s])
  if (todo.length === 0) return null
  return { done, todo, count: STEPS.length - todo.length, total: STEPS.length, next: todo[0], dismiss: () => setDismissed(true), confirmProfile: () => setConfirmed(true) }
}

/**
 * "Set up 2 of 5" and the steps still to do, each a tap to where it is done; the first is the next thing to do.
 * `bar`: one line on the Camera tab, the steps scroll sideways when they do not fit. `pill`: collapsed to one chip on the other tabs, opening the same steps.
 * Never blocks anything; the x hides it for good.
 */
export function SetupStrip({ status, onStep, variant }: { status: Status | null; onStep: (id: SetupStepId) => void; variant: 'bar' | 'pill' }) {
  const setup = useSetup(status)
  const pop = usePopover()
  if (!setup || !status) return null
  const label = `Set up ${setup.count} of ${setup.total}`
  const hide = () => { pop.setOpen(false); setup.dismiss() }
  const profile = PROFILES.find((x) => x.id === profileOf(status))?.label ?? ''
  const steps = setup.todo.map((s) => {
    // on the Camera tab the profile step is the confirmation itself (the picker is right below); elsewhere it goes there
    const keep = s === 'profile' && variant === 'bar'
    return (
      <button key={s} type="button" className={s === setup.next ? 'primary' : ''} onClick={() => { pop.setOpen(false); if (keep) setup.confirmProfile(); else onStep(s) }}>
        <span className="ss-n" aria-hidden="true">{STEPS.indexOf(s) + 1}</span>{keep ? `Keep ${profile}` : stepLabel(s, status)}
      </button>
    )
  })
  if (variant === 'bar') {
    return (
      <nav className="setup-strip" aria-label="First session set up">
        <span className="ss-label">{label}</span>
        <div className="ss-steps">{steps}</div>
        <button type="button" className="quiet ss-x" aria-label="Hide the set up steps" onClick={hide}>
          <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><path d="M3 3l10 10M13 3L3 13" stroke="currentColor" strokeWidth="2" strokeLinecap="round" /></svg>
        </button>
      </nav>
    )
  }
  return (
    <div ref={pop.box} className="setup-pill">
      <button ref={pop.button} type="button" className="chip info setup-chip" aria-expanded={pop.open} aria-controls="setup-sheet" onClick={() => pop.setOpen(!pop.open)}>
        <ToneIcon tone="off" /><span className="cl">{label}</span>
      </button>
      {pop.open && (
        <div className="notes-sheet" id="setup-sheet" role="region" aria-label="Set up steps">
          <p>Still to do, in order. Nothing is blocked: do them when you need them.</p>
          <div className="ss-list">{steps}</div>
          <div className="btnrow tight"><button type="button" className="small" onClick={hide}>Hide the set up steps</button></div>
        </div>
      )}
    </div>
  )
}
