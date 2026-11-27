import { ReactNode } from 'react'

/** The browser only shares its location on a secure page (HTTPS or localhost); the Pi is normally served over plain http://pi.local. */
export const canLocate = () => typeof window !== 'undefined' && window.isSecureContext === true && 'geolocation' in navigator

export const NEEDS_HTTPS = 'Using this phone’s location needs HTTPS, and this page is plain http. Type the latitude and longitude instead, or set up HTTPS once (Phone setup in the guide: run sudo rustflare-tls on the Pi).'

/** Asks the browser where it is; a refusal says what to do next, not only what went wrong. */
export function locate(): Promise<{ latitude: number; longitude: number }> {
  return new Promise((resolve, reject) => {
    if (!canLocate()) { reject(new Error(NEEDS_HTTPS)); return }
    navigator.geolocation.getCurrentPosition(
      (p) => resolve({ latitude: p.coords.latitude, longitude: p.coords.longitude }),
      (e) => reject(new Error(`The phone did not share its location (${e.message}). Allow location for this page in the browser settings and try again, or type the latitude and longitude.`)),
      { timeout: 15000 },
    )
  })
}

/**
 * The two ways to set the site. On a page that cannot ask for the location (plain http) "Type it in" is the main action and the
 * location button is not offered; the line under it says why and how to get it.
 */
export function SiteChoices({ busy, onUse, onType, label = 'Use my location' }: { busy: boolean; onUse: () => void; onType?: () => void; label?: string }): ReactNode {
  const secure = canLocate()
  return (
    <>
      <div className="btnrow tight">
        {secure && <button type="button" className="primary grow" disabled={busy} onClick={onUse}>{label}</button>}
        {onType && <button type="button" className={`grow ${secure ? '' : 'primary'}`} onClick={onType}>Type it in</button>}
      </div>
      {!secure && <p className="hint-text" role="note">Using this phone’s location needs HTTPS, and this page is plain http. See Phone setup in the guide (run <code>sudo rustflare-tls</code> on the Pi).</p>}
    </>
  )
}
