import { CameraStatus } from './api'
import { Lane, PRESETS, parseLanes } from './components/BracketLanes'
import { getSetting } from './settings'

export type RecordKind = 'plain' | 'prominence' | 'hdr'
export interface Recipe {
  kind: RecordKind; name: string; format: 'ser' | 'png' | 'fits'; onFull: 'wait' | 'drop'
  frames: string; seconds: string; every: string; repeats: string; factor: string; mask: boolean; lanes: Lane[]
  /** Send the mount home when the recording finishes by itself. */
  home: boolean
}

const n = (s: string) => (s.trim() === '' ? undefined : parseFloat(s))

/** The recording recipe as last set on the Record tab. */
export function savedRecipe(): Recipe {
  return {
    kind: getSetting<RecordKind>('capture.kind', 'plain'), name: getSetting('capture.name', ''), format: getSetting('capture.format', 'ser'), onFull: getSetting('capture.on_full', 'wait'),
    frames: getSetting('capture.frames', '100'), seconds: getSetting('capture.seconds', ''), every: getSetting('capture.every', ''), repeats: getSetting('capture.bursts', ''),
    factor: getSetting('capture.promvid_factor', '8'), mask: getSetting('capture.promvid_mask', false), lanes: parseLanes(getSetting('capture.lanes', JSON.stringify(PRESETS['Partial phase']))),
    home: getSetting('capture.home_when_done', false),
  }
}

/** The API call that starts a recording with this recipe. */
export function recordRequest(r: Recipe, cam: CameraStatus): { path: string; body: Record<string, unknown> } {
  const nm = r.name.trim() || undefined
  if (r.kind === 'plain') {
    return r.every.trim() !== ''
      ? { path: '/capture/bursts', body: { name: nm, frames: n(r.frames), seconds: n(r.seconds), interval_s: n(r.every), bursts: n(r.repeats), format: r.format, on_full: r.onFull, home_when_done: r.home } }
      : { path: '/capture/sequence', body: { name: nm, frames: n(r.frames), seconds: n(r.seconds), format: r.format, on_full: r.onFull, home_when_done: r.home } }
  }
  const exp = cam.controls.find((x) => x.id === 'exposure_us')?.value
  const f = parseFloat(r.factor) || 8
  const list = r.kind === 'prominence'
    ? [{ name: 'disc', exposure_us: exp, frames: n(r.frames), seconds: n(r.seconds) }, { name: 'prominence', exposure_us: (exp ?? 0) * f, frames: n(r.frames), seconds: n(r.seconds), mask_disc: r.mask }]
    : r.lanes.map((l) => ({ name: l.name, exposure_us: (parseFloat(l.exposure_ms) || 0) * 1000, gain: l.gain ? parseFloat(l.gain) : undefined, frames: parseInt(l.frames, 10) || undefined }))
  return { path: '/capture/lanes', body: { name: nm, kind: r.kind === 'hdr' ? 'hdr' : 'prominence', cycles: n(r.repeats), cycle_interval_s: n(r.every), format: r.format === 'png' ? 'ser' : r.format, on_full: r.onFull, home_when_done: r.home, lanes: list } }
}

export const recipeLabel = (r: Recipe) => (r.kind === 'plain' ? (r.every.trim() ? 'repeating recording' : 'recording') : r.kind === 'prominence' ? 'prominence recording' : 'HDR lanes')
