import { useEffect, useState } from 'react'
import { api, reportError } from './api'

/** The preview's auto-stretch is one setting for everyone watching; the overlay button and More > Screen show and change the same one. */
let value = false
let loaded = false
const listeners = new Set<() => void>()
const emit = () => listeners.forEach((l) => l())

function load() {
  api<{ stretch: boolean }>('GET', '/preview')
    .then((r) => { if (r.stretch !== value || !loaded) { value = r.stretch; loaded = true; emit() } })
    .catch((e) => reportError(e, 'Reading the auto-stretch setting'))
}

export function useStretch(): [boolean, (on: boolean) => void] {
  const [, force] = useState(0)
  useEffect(() => {
    const l = () => force((n) => n + 1)
    listeners.add(l)
    if (!loaded) load()
    window.addEventListener('focus', load)
    return () => { listeners.delete(l); window.removeEventListener('focus', load) }
  }, [])
  const set = (on: boolean) => {
    value = on // shown at once; the server answer below confirms or puts it back
    emit()
    api('PUT', '/preview', { stretch: on }).catch((e) => { reportError(e, 'Changing auto-stretch'); load() })
  }
  return [value, set]
}
