import { Status } from './api'

/** What is being imaged. The profile changes words and what is enabled; it never hides a control and the API refuses nothing. */
export type ProfileId = 'sun_white' | 'sun_ha' | 'moon' | 'planet' | 'other'

export const PROFILES: { id: ProfileId; label: string; blurb: string }[] = [
  { id: 'sun_white', label: 'Sun, white light', blurb: 'The Sun with a white-light filter: disc, sunspots and granulation.' },
  { id: 'sun_ha', label: 'Sun, H-alpha', blurb: 'The Sun in H-alpha: disc, filaments and prominences (coronagraph and prominence lanes).' },
  { id: 'moon', label: 'Moon', blurb: 'The Moon, whole or in part: lunar tracking, craters as tracking features.' },
  { id: 'planet', label: 'Planet', blurb: 'A planet: a small bright disc on a dark sky. Star-rate tracking; the Sun tools are switched off.' },
  { id: 'other', label: 'Other', blurb: 'Anything else: nothing is assumed about the target.' },
]

export const profileOf = (status: Status | null | undefined): ProfileId => (status?.profile as ProfileId | undefined) ?? 'sun_ha'
export const isSun = (p: ProfileId) => p === 'sun_white' || p === 'sun_ha'

/** The noun the copy uses for the thing in the picture. */
export function noun(p: ProfileId): string {
  return p === 'moon' ? 'Moon' : p === 'planet' ? 'planet' : p === 'other' ? 'target' : 'Sun'
}

/** Things a profile can switch off. `why` gives the reason shown under the greyed control, or null when it is available. */
export type Feature = 'corona' | 'prominence' | 'limb' | 'edgeCircle' | 'lanes' | 'flip'

const WHY: Record<Feature, { not: ProfileId[]; text: string }> = {
  corona: { not: ['moon', 'planet', 'other'], text: "Prominence view hides the Sun's disc. It does nothing for the Moon or a planet." },
  prominence: { not: ['moon', 'planet', 'other'], text: 'Prominence recordings pair a disc exposure with a long exposure of the Sun\'s edge. Not used for the Moon or planets.' },
  limb: { not: ['planet'], text: 'A planet is only a few tens of pixels across: there is no edge to fit. Use Feature, or track by hand with the slew pad.' },
  edgeCircle: { not: ['planet'], text: 'No edge circle: a planet is too small for a limb circle to be worth showing.' },
  lanes: { not: ['planet'], text: 'Exposure lanes bracket a bright and a dark scene (an eclipse, the Moon). A planet needs one exposure.' },
  flip: { not: ['moon', 'planet', 'other'], text: 'The automatic flip and the flip assist follow the Sun for now. For the Moon or a planet flip by hand: Go to the target again after the meridian.' },
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
