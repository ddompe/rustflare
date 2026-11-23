import { useEffect, useState } from 'react'

export interface Control { id: string; label: string; min: number; max: number; step: number; value: number; unit: string | null; log: boolean }
export interface CameraStatus { driver: string; id: string; name: string; controls: Control[]; temperature_c: number | null; width: number; height: number; bits: number; bayer?: string | null; fps: number; mean: number; saturated: number; histogram: number[] }
export interface Tracking { mode: string; x: number; y: number; radius: number | null; lost: boolean }
export interface Calibration { primary_dir: { x: number; y: number }; secondary_dir: { x: number; y: number } }
export interface Optics { focal_length_mm: number | null; focal_multiplier: number; effective_focal_mm: number | null; pixel_size_override_um: number | null; pixel_size_um: number | null; pixel_size_source: 'camera' | 'manual' | null; binning: number; scale_arcsec_per_px: number | null; solar_radius_arcsec: number; expected_radius_px: number | null
  /** Measured from the limb; all null/absent until a partial or whole disc has been measured. */
  measured_radius_px?: number | null; suggested_effective_focal_mm?: number | null; suggested_multiplier?: number | null }
export interface StorageInfo { path: string; label: string; mount_point: string | null; fstype: string | null; total: number | null; free: number | null; max_file_bytes: number | null; available: boolean; problem: string | null }
export interface SunLocation { pos: { x: number; y: number }; radius: number; partial: boolean; radius_from_optics?: boolean | null; fit_rms?: number | null }
export interface Startup { enabled: boolean; camera: string | null; mount: string | null; retrying: boolean }
export interface Status {
  site?: { latitude: number; longitude: number } | null
  sky_targets?: { id: string; name: string; altitude_deg: number; azimuth_deg: number; ra_hours: number; dec_deg: number }[]
  sky_sun?: { altitude_deg: number; azimuth_deg: number; ra_hours: number; dec_deg: number; hour_angle_h: number; minutes_to_meridian: number } | null
  clock?: { ntp_synchronized: boolean | null }
  goto?: { target: string; phase: string } | null
  flip?: { phase: string; elapsed_s: number; detail: string } | null
  sun_held?: boolean
  auto_flip?: { enabled: boolean; record_min: number; flip_min: number; stage: 'off' | 'waiting' | 'recorded' | 'flipping' | 'done'; message: string; resume_pending: boolean }
  last_flip?: unknown
  guide_params?: { window_s: number; deadband_px: number; gain: number; speed: number; fast_speed: number; fast_above_px: number }
  coronagraph?: { enabled: boolean; scale: number }
  focus?: { enabled: boolean; live: number | null; lucky: number | null; lucky_err: number | null; best: number | null; best_err: number | null; best_age_s: number | null; history: [number, number, number][]; note: string | null }
  colour?: { mode: string; r: number; b: number; applied: [number, number, number] }
  session_log?: { lost_frames: boolean; lost_saved: number; enabled: boolean; lines: number; frames: number; bytes: number; file: string | null }
  roi?: { x: number; y: number; width: number; height: number; sensor_width: number; sensor_height: number } | null
  darks?: { available: boolean; applied: boolean; name: string | null; exposure_us: number | null; gain: number | null; matches_camera: boolean }
  flats?: { applied: boolean; available: boolean; name: string | null }
  startup: Startup
  optics: Optics
  camera: CameraStatus | null
  sun: SunLocation | null
  /** What the mount itself reports (read back every ~2 s); everything but info and tracking may be null. */
  mount: { info: string; tracking: boolean; rate?: 'solar' | 'lunar' | 'sidereal'; slewing?: boolean | null; at_home?: boolean | null; park?: 'unparked' | 'parking' | 'parked' | null; alt_az?: boolean | null; link?: 'usb' | 'ble' | null; link_up?: boolean } | null
  tracking: Tracking | null
  guiding: { state: 'off' | 'calibrating' | 'guiding'; calibration: Calibration | null; target: [number, number] | null; error_px: number | null; held?: boolean; dead_reckoning?: boolean }
  capture: {
    state: 'idle' | 'single' | 'sequence' | 'bursts' | 'flats' | 'flushing'
    phase: string; done: number; total: number | null; file: string | null; last_file: string | null
    burst: number | null; bursts_total: number | null; next_in_s: number | null
    dropped?: number; paused?: boolean; paused_reason?: string | null; stopped_early?: string | null; frames_lost?: string | null; eta_s?: number | null; buffer_queued: number; buffer_capacity: number; buffer_mb: number; ram_available_mb: number | null; disk_mb_s: number
  }
  after_burst?: AfterBurst
  notices?: Notice[]
  error: string | null
  seq: number
}
/** Something to know about, shown until dismissed. */
export interface Notice { id: number; level: 'error' | 'warning'; text: string; at: string; acked: boolean }
export type AfterMode = 'video' | 'video_stack' | 'stack_only' | 'best_frames'
/** What happens after each burst, and whether the stacker is there to do it. */
export interface AfterBurst { mode: AfterMode; stack_keep_percent: number; raw_keep_percent: number; margin: number; stacker_connected: boolean; stacking: number; stacked: number; last: string | null; stack_s_per_frame: number }
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
