import { useEffect, useState } from 'react'
import { api, Status } from './api'

/**
 * Device datasheets for the camera and mount "device cards".
 *
 * Contract (planned server endpoints, not implemented yet):
 *   GET /api/datasheet/camera, GET /api/datasheet/mount  ->  { live, catalog }
 * Every field is optional: the UI shows what is present and nothing else. Until the endpoints exist,
 * `fetchDatasheet` (the one place that talks to them) falls back to a stub built from the live `Status`,
 * with no catalog. Add `?datasheet=demo` to the page URL to preview the cards with placeholder catalog values.
 */
export interface CameraLive { model?: string; sensor?: string; width?: number; height?: number; pixel_um?: number; bits?: number; color?: boolean; cooled?: boolean; serial?: string }
export interface CameraCatalog {
  vendor?: string; model?: string; sensor?: string; sensor_size_mm?: string; resolution?: string; pixel_um?: number; adc_bits?: number
  max_fps_full_res?: number; full_well_e?: number; read_noise_e?: number
  /** Gain at which the high-conversion-gain mode switches on. */
  hcg_gain?: number | null; hcg_note?: string; unity_gain?: number; qe_peak_pct?: number
  notes?: string; sources?: string[]; image?: string
}
export interface MountLive { model?: string; port?: string; tracking?: boolean }
export interface MountCatalog {
  vendor?: string; model?: string; payload_kg?: number; weight_kg?: number; modes?: string[]; guide_rates?: string | string[]
  tracking_rates?: string[]; power?: string; ports?: string[]; notes?: string; sources?: string[]; image?: string
}
export interface Datasheets {
  camera: { live?: CameraLive; catalog?: CameraCatalog | null }
  mount: { live?: MountLive; catalog?: MountCatalog | null }
}
export type DeviceKind = keyof Datasheets

const DEMO_NOTE = 'Placeholder values for previewing the card, not manufacturer data.'
const DEMO: { [K in DeviceKind]: Datasheets[K]['catalog'] } = {
  camera: { vendor: 'Example', model: 'Demo camera', sensor: 'Example sensor', resolution: '1944 × 1472', pixel_um: 2.9, adc_bits: 12, max_fps_full_res: 57, full_well_e: 12000, read_noise_e: 1.2, hcg_gain: 120, hcg_note: 'High conversion gain switches on here (placeholder).', unity_gain: 60, qe_peak_pct: 80, notes: DEMO_NOTE, sources: ['(placeholder)'] },
  mount: { vendor: 'Example', model: 'Demo mount', payload_kg: 10, weight_kg: 4, modes: ['Equatorial', 'Alt-az'], tracking_rates: ['Sidereal', 'Solar', 'Lunar'], power: '12-24 V DC', ports: ['USB-C', 'ST-4'], notes: DEMO_NOTE, sources: ['(placeholder)'] },
}

function stub<K extends DeviceKind>(kind: K, status: Status | null): Datasheets[K] {
  const demo = new URLSearchParams(location.search).get('datasheet') === 'demo'
  if (kind === 'camera') {
    const c = status?.camera
    const live: CameraLive = c ? { model: c.name, width: c.width, height: c.height, bits: c.bits, pixel_um: status?.optics.pixel_size_um ?? undefined } : {}
    return { live, catalog: demo ? DEMO.camera : null } as Datasheets[K]
  }
  const m = status?.mount
  return { live: m ? { model: m.info, tracking: m.tracking } : {}, catalog: demo ? DEMO.mount : null } as Datasheets[K]
}

/** The single entry point: swap the body for a plain fetch result once the server provides the endpoint. */
export async function fetchDatasheet<K extends DeviceKind>(kind: K, status: Status | null): Promise<Datasheets[K]> {
  try {
    const r = await api<Datasheets[K]>('GET', `/datasheet/${kind}`)
    if (r && typeof r === 'object') return r
  } catch { /* endpoint not available yet */ }
  return stub(kind, status)
}

/** Loads the datasheet when a device appears, and again when it changes. Never throws; always returns something. */
export function useDatasheet<K extends DeviceKind>(kind: K, status: Status | null, deviceKey: string | null): Datasheets[K] {
  const [data, setData] = useState<Datasheets[K]>(() => stub(kind, status))
  useEffect(() => {
    if (!deviceKey) return
    let live = true
    fetchDatasheet(kind, status).then((d) => { if (live) setData(d) })
    return () => { live = false }
  }, [kind, deviceKey]) // eslint-disable-line react-hooks/exhaustive-deps
  // live facts fall back to the current status for any field the server left out
  const fresh = stub(kind, status)
  return { ...data, live: { ...fresh.live, ...data.live } } as Datasheets[K]
}

/** Original line art ships in web/public/devices; a catalog `image` filename overrides it (e.g. a licensed photo). */
export function deviceImage(kind: DeviceKind, catalogImage?: string | null): string {
  if (catalogImage) return '/devices/' + catalogImage.replace(/^.*\//, '')
  return `/devices/${kind}.svg`
}
