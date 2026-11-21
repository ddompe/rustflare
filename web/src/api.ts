import { useEffect, useState } from 'react'

export interface Control { id: string; label: string; min: number; max: number; step: number; value: number; unit: string | null; log: boolean }
export interface CameraStatus { driver: string; id: string; name: string; controls: Control[]; temperature_c: number | null; width: number; height: number; bits: number; fps: number; mean: number; saturated: number; histogram: number[] }
export interface Tracking { mode: string; x: number; y: number; radius: number | null; lost: boolean }
export interface Calibration { primary_dir: { x: number; y: number }; secondary_dir: { x: number; y: number } }
export interface Optics { focal_length_mm: number | null; focal_multiplier: number; effective_focal_mm: number | null; pixel_size_override_um: number | null; pixel_size_um: number | null; pixel_size_source: 'camera' | 'manual' | null; binning: number; scale_arcsec_per_px: number | null; solar_radius_arcsec: number; expected_radius_px: number | null }
export interface StorageInfo { path: string; label: string; mount_point: string | null; fstype: string | null; total: number | null; free: number | null; max_file_bytes: number | null; available: boolean; problem: string | null }
export interface SunLocation { pos: { x: number; y: number }; radius: number; partial: boolean }
export interface Startup { enabled: boolean; camera: string | null; mount: string | null; retrying: boolean }
export interface Status {
  startup: Startup
  optics: Optics
  camera: CameraStatus | null
  sun: SunLocation | null
  mount: { info: string; tracking: boolean } | null
  tracking: Tracking | null
  guiding: { state: 'off' | 'calibrating' | 'guiding'; calibration: Calibration | null; target: [number, number] | null; error_px: number | null }
  capture: {
    state: 'idle' | 'single' | 'sequence' | 'bursts' | 'flats' | 'flushing'
    phase: string; done: number; total: number | null; file: string | null; last_file: string | null
    burst: number | null; bursts_total: number | null; next_in_s: number | null
    buffer_queued: number; buffer_capacity: number; buffer_mb: number; ram_available_mb: number | null; disk_mb_s: number
  }
  error: string | null
  seq: number
}
export interface CameraInfo { driver: string; id: string; name: string }

export async function api<T = unknown>(method: 'GET' | 'POST' | 'PUT', path: string, body?: unknown): Promise<T> {
  const r = await fetch('/api' + path, {
    method,
    headers: body === undefined ? undefined : { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  const j = await r.json().catch(() => ({}))
  if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText)
  return j as T
}

/** Live status over WebSocket with automatic reconnect. */
export function useStatus(): { status: Status | null; connected: boolean } {
  const [status, setStatus] = useState<Status | null>(null)
  const [connected, setConnected] = useState(false)
  useEffect(() => {
    let ws: WebSocket | null = null
    let timer: number | undefined
    let closed = false
    const open = () => {
      ws = new WebSocket(`${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/api/ws`)
      ws.onopen = () => setConnected(true)
      ws.onmessage = (e) => setStatus(JSON.parse(e.data))
      ws.onclose = () => {
        setConnected(false)
        if (!closed) timer = window.setTimeout(open, 1000)
      }
    }
    open()
    return () => {
      closed = true
      window.clearTimeout(timer)
      ws?.close()
    }
  }, [])
  return { status, connected }
}
