import { Status } from './api'

/** What is being imaged. The profile changes words and what is enabled; it never hides a control and the API refuses nothing. */
export type ProfileId = 'sun_white' | 'sun_ha' | 'moon' | 'planet' | 'other'

export const PROFILES: { id: ProfileId; label: string; /** one line, under the picker */ short: string; /** the longer sentence, behind "What is this?" */ blurb: string; /** the tracking rate that suits it, for the one-tap offer on Aim (null: nothing assumed) */ rate: 'solar' | 'lunar' | 'sidereal' | null }[] = [
  { id: 'sun_white', label: 'Sun, white light', short: 'Disc, sunspots, granulation', blurb: 'The Sun with a white-light filter: disc, sunspots and granulation.', rate: 'solar' },
  { id: 'sun_ha', label: 'Sun, H-alpha', short: 'Disc, filaments, prominences', blurb: 'The Sun in H-alpha: disc, filaments and prominences (coronagraph and prominence lanes).', rate: 'solar' },
  { id: 'moon', label: 'Moon', short: 'Whole or part of the Moon', blurb: 'The Moon, whole or in part. Craters work as tracking features. The profile does not change the mount\u2019s tracking rate: Aim offers the lunar rate.', rate: 'lunar' },
  { id: 'planet', label: 'Planet', short: 'Small bright disc, Sun tools off', blurb: 'A planet: a small bright disc on a dark sky. The Sun tools are switched off. The profile does not change the mount\u2019s tracking rate: Aim offers the star rate.', rate: 'sidereal' },
  { id: 'other', label: 'Other', short: 'Nothing is assumed', blurb: 'Anything else: nothing is assumed about the target.', rate: null },
]

export const profileOf = (status: Status | null | undefined): ProfileId => (status?.profile as ProfileId | undefined) ?? 'sun_ha'
export const isSun = (p: ProfileId) => p === 'sun_white' || p === 'sun_ha'

/** The noun the copy uses for the thing in the picture. */
export function noun(p: ProfileId): string {
  return p === 'moon' ? 'Moon' : p === 'planet' ? 'planet' : p === 'other' ? 'target' : 'Sun'
}

/** Things a profile can switch off. `why` gives the reason shown under the greyed control, or null when it is available. */
export type Feature = 'corona' | 'prominence' | 'limb' | 'edgeCircle' | 'lanes' | 'flip' | 'planetTracker'

const WHY: Record<Feature, { not: ProfileId[]; text: string }> = {
  corona: { not: ['moon', 'planet', 'other'], text: "it blacks out the Sun's disc, so it is for the Sun only." },
  prominence: { not: ['moon', 'planet', 'other'], text: 'it pairs a disc exposure with a long one of the Sun\u2019s edge: Sun only.' },
  limb: { not: ['planet'], text: 'a planet has no edge to fit: use Feature, or the slew pad.' },
  edgeCircle: { not: ['planet'], text: 'a planet is too small for an edge circle.' },
  lanes: { not: ['planet'], text: 'lanes bracket a bright and a dark scene (eclipse, Moon); a planet needs one exposure.' },
  planetTracker: { not: ['sun_white', 'sun_ha', 'moon', 'other'], text: 'it needs the Planet profile; use Feature or Disc edge for a larger target.' },
  flip: { not: ['other'], text: 'pick Sun, Moon or Planet so the flip knows which body to follow.' },
}

export function why(p: ProfileId, f: Feature): string | null {
  return WHY[f].not.includes(p) ? WHY[f].text : null
}

/** The stacker on the Pi handles mono recordings only: a fact about the camera, true for every profile. */
export function whyStack(status: Status | null | undefined): string | null {
  return status?.camera?.bayer ? 'The Pi stacker only handles mono recordings. Record the raw colour video and stack it in AutoStakkert, PIPP or Siril.' : null
}

/** The profile a Go to target belongs to, so Aim can offer the switch. */
export function profileForTarget(target: string): ProfileId | null {
  return target === 'sun' ? null : target === 'moon' ? 'moon' : 'planet'
}

/**
 * Clipping as the profile judges it. A planet covers a tenth of a percent of the frame, so "more than 0.1 % of the pixels at full scale"
 * can never be true of it before the whole planet is white: for the planet profile the brightest part of the picture (status
 * `camera.peak`, the 99.99th percentile) is what is checked.
 */
export function clipping(cam: { saturated: number; peak?: number }, p: ProfileId): { clipped: boolean; value: string; label: string; help: string } {
  if (p === 'planet') {
    const peak = cam.peak ?? 0
    return { clipped: peak >= 0.98, value: `${Math.round(peak * 100)}%`, label: 'planet at full scale', help: 'The brightest part of the picture is at full scale: reduce exposure or gain' }
  }
  return { clipped: cam.saturated > 0.001, value: `${(cam.saturated * 100).toFixed(1)}%`, label: 'clipping', help: 'Pixels at full scale: reduce exposure or gain' }
}
