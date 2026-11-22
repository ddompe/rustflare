import { useCallback, useState } from 'react'
import { api } from './api'

/**
 * UI preferences (slew speed, form fields, display options, ...) stored on the Pi, not in the browser:
 * they survive reboots and follow you between phone and laptop.
 *
 * `loadSettings()` runs once before the app renders (main.tsx), so components start with the saved value
 * instead of flashing the default. `useSetting` behaves like `useState` and saves changes (debounced).
 */
let cache: Record<string, unknown> = {}

export async function loadSettings(): Promise<void> {
  try {
    cache = await api<Record<string, unknown>>('GET', '/ui')
  } catch {
    cache = {} // server unreachable: fall back to defaults
  }
}

const timers: Record<string, number> = {}
function save(key: string, value: unknown) {
  window.clearTimeout(timers[key])
  timers[key] = window.setTimeout(() => { api('PUT', '/ui', { key, value }).catch(() => {}) }, 400)
}

/** Reads a saved setting outside a component (the quick Rec button reuses the Record tab's last recipe). */
export function getSetting<T>(key: string, def: T): T {
  const v = cache[key]
  return v === undefined || v === null ? def : (v as T)
}

export function useSetting<T>(key: string, def: T): [T, (v: T) => void] {
  const [value, setValue] = useState<T>(() => {
    const stored = cache[key]
    if (stored === undefined || stored === null) return def
    // objects are merged over the defaults so newly added fields get their default
    return typeof def === 'object' && def !== null && !Array.isArray(def) ? ({ ...def, ...(stored as object) } as T) : (stored as T)
  })
  const set = useCallback((v: T) => { setValue(v); cache[key] = v; save(key, v) }, [key])
  return [value, set]
}
