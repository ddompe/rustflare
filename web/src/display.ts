import { useEffect, useState } from 'react'

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
  grid: boolean // rule-of-thirds
  sunOutline: boolean
  zoom: number
  panX: number
  panY: number
}

export const DEFAULT_DISPLAY: Display = {
  invert: false, tint: 'none', brightness: 1, contrast: 1, gamma: 1, fps: 15,
  crosshair: true, rings: 2, grid: false, sunOutline: true, zoom: 1, panX: 0, panY: 0,
}

const KEY = 'rustflare.display'

export function useDisplay(): [Display, (patch: Partial<Display>) => void, () => void] {
  const [d, setD] = useState<Display>(() => {
    try { return { ...DEFAULT_DISPLAY, ...JSON.parse(localStorage.getItem(KEY) ?? '{}'), zoom: 1, panX: 0, panY: 0 } } catch { return DEFAULT_DISPLAY }
  })
  useEffect(() => { localStorage.setItem(KEY, JSON.stringify(d)) }, [d])
  return [d, (p) => setD((x) => ({ ...x, ...p })), () => setD(DEFAULT_DISPLAY)]
}

/** CSS filter chain for the image; the SVG filter `#rf-look` (see LiveView) does gamma and tint. */
export function cssFilter(d: Display): string {
  return `brightness(${d.brightness}) contrast(${d.contrast}) url(#rf-gamma)${d.invert ? ' invert(1)' : ''} url(#rf-tint-${d.tint})`
}
