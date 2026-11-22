import { useState } from 'react'
import { useSetting } from './settings'

/** How the live image is presented in the browser. Purely client-side except `stretch` (server) and `fps` (stream). */
export interface Display {
  invert: boolean
  tint: 'none' | 'orange' | 'red'
  brightness: number
  contrast: number
  gamma: number
  fps: number
  crosshair: boolean
  rings: number // concentric "bullseye" rings, 0 = none
  ringSize: number // radius of the outermost ring, % of the frame height
  grid: boolean // rule-of-thirds
  sunOutline: boolean // the cyan edge circle
  sunCentre: boolean // the centre dot and the arrow to the frame centre
  overlays: boolean // master switch: false hides every drawn overlay so the bare image shows
  zoom: number
  panX: number
  panY: number
}

export const DEFAULT_DISPLAY: Display = {
  invert: false, tint: 'none', brightness: 1, contrast: 1, gamma: 1, fps: 15,
  crosshair: true, rings: 3, ringSize: 15, grid: false, sunOutline: true, sunCentre: true, overlays: true, zoom: 1, panX: 0, panY: 0,
}

type Prefs = Omit<Display, 'zoom' | 'panX' | 'panY'>
const { zoom: _z, panX: _x, panY: _y, ...DEFAULT_PREFS } = DEFAULT_DISPLAY
void _z; void _x; void _y

/** Display preferences live on the Pi (see settings.ts); zoom and pan are per-visit and always start reset. */
export function useDisplay(): [Display, (patch: Partial<Display>) => void, () => void] {
  const [prefs, setPrefs] = useSetting<Prefs>('display', DEFAULT_PREFS)
  const [view, setView] = useState({ zoom: 1, panX: 0, panY: 0 })
  const set = (p: Partial<Display>) => {
    const { zoom, panX, panY, ...rest } = p
    if (zoom !== undefined || panX !== undefined || panY !== undefined) {
      setView((v) => ({ zoom: zoom ?? v.zoom, panX: panX ?? v.panX, panY: panY ?? v.panY }))
    }
    if (Object.keys(rest).length) setPrefs({ ...prefs, ...rest })
  }
  const reset = () => { setPrefs(DEFAULT_PREFS); setView({ zoom: 1, panX: 0, panY: 0 }) }
  return [{ ...prefs, ...view }, set, reset]
}

/** CSS filter chain for the image; the SVG filter `#rf-look` (see LiveView) does gamma and tint. */
export function cssFilter(d: Display): string {
  return `brightness(${d.brightness}) contrast(${d.contrast}) url(#rf-gamma)${d.invert ? ' invert(1)' : ''} url(#rf-tint-${d.tint})`
}
