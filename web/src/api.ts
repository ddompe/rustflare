import { useEffect, useState } from 'react'

export interface Control { id: string; label: string; min: number; max: number; step: number; value: number; unit: string | null; log: boolean }
export interface Cooler { available: boolean; on: boolean; target_c: number | null; target_min_c: number | null; target_max_c: number | null; power_pct: number | null; temp_c: number | null; fan: boolean | null; heater: boolean | null; warming_up: boolean }
export interface CameraStatus { driver: string; id: string; name: string; controls: Control[]; temperature_c: number | null; cooler?: Cooler; width: number; height: number; bits: number; bayer?: string | null; fps: number; mean: number; saturated: number; peak?: number; histogram: number[]; histogram_rgb?: number[][]; saturated_rgb?: number[] }
export interface Tracking { mode: string; x: number; y: number; radius: number | null; lost: boolean }
export interface Calibration { primary_dir: { x: number; y: number }; secondary_dir: { x: number; y: number } }
export interface Optics { focal_length_mm: number | null; focal_multiplier: number; effective_focal_mm: number | null; pixel_size_override_um: number | null; pixel_size_um: number | null; pixel_size_source: 'camera' | 'manual' | null; binning: number; scale_arcsec_per_px: number | null; solar_radius_arcsec: number; expected_radius_px: number | null
  /** Measured from the limb; all null/absent until a partial or whole disc has been measured. */
  /** Apparent radius of what the profile images, arcseconds (the Sun's, the Moon's, a planet's globe). */
  target_radius_arcsec?: number | null
  measured_radius_px?: number | null; suggested_effective_focal_mm?: number | null; suggested_multiplier?: number | null
  /** The named optical setup in use, and whether the optics or the profile were changed after it was saved. */
  setup?: { active: string | null; modified: boolean } }
export interface StorageInfo { path: string; label: string; mount_point: string | null; fstype: string | null; total: number | null; free: number | null; max_file_bytes: number | null; available: boolean; problem: string | null; internal?: boolean; reserve_bytes?: number; warning?: string | null }
export interface SunLocation { pos: { x: number; y: number }; radius: number; partial: boolean; radius_from_optics?: boolean | null; fit_rms?: number | null }
/** A site and a clock: the mount's own or the Pi's. Null fields are unknown (the mount does not say, or no site is saved). */
export interface ClockPlace { latitude_deg: number | null; longitude_east_deg: number | null; unix_secs: number | null; utc_offset_minutes: number | null }
/** What the mount holds as its site and clock against the Pi's (null: this mount cannot report them, or nothing was read yet). */
export interface MountSiteTime {
  read_at_unix: number; mount: ClockPlace; pi: ClockPlace
  /** Mount clock minus the Pi's, seconds. */
  time_diff_s: number | null; place_diff_deg: number | null
  time_ok: boolean | null; place_ok: boolean | null; ok: boolean; why: string
}
export interface Startup { enabled: boolean; camera: string | null; mount: string | null; retrying: boolean }
export interface Status {
  /** The software version (a local build adds its commit): shown in the corner of the status bar. */
  version?: string
  site?: { latitude: number; longitude: number } | null
  sky_targets?: { id: string; name: string; altitude_deg: number; azimuth_deg: number; ra_hours: number; dec_deg: number }[]
  sky_sun?: { altitude_deg: number; azimuth_deg: number; ra_hours: number; dec_deg: number; hour_angle_h: number; minutes_to_meridian: number } | null
  /** What the profile images (null for Other, and for Planet until a planet was gone to). */
  target?: { id: string; name: string } | null
  /** The body the meridian flip follows, with its own meridian timing and the tracking rate restored after the turn. */
  flip_target?: { id: string; name: string; ra_hours: number; dec_deg: number; hour_angle_h: number; minutes_to_meridian: number; rate: 'solar' | 'lunar' | 'sidereal' } | null
  clock?: { ntp_synchronized: boolean | null }
  goto?: { target: string; phase: string } | null
  flip?: { phase: string; elapsed_s: number; detail: string } | null
  sun_held?: boolean
  auto_flip?: { enabled: boolean; record_min: number; flip_min: number; stage: 'off' | 'waiting' | 'recorded' | 'flipping' | 'done' | 'late'; message: string; resume_pending: boolean }
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
  /** What is being imaged: sun_white, sun_ha, moon, planet or other (engine-owned, see profile.ts). */
  profile?: string
  /** What the mount itself reports (read back every ~2 s); everything but info and tracking may be null. */
  mount: { info: string; slew_speeds?: number[]; auto_sync_site_time?: boolean; can_search_home?: boolean; tracking: boolean; rate?: 'solar' | 'lunar' | 'sidereal'; slewing?: boolean | null; at_home?: boolean | null; park?: 'unparked' | 'parking' | 'parked' | null; alt_az?: boolean | null; link?: 'usb' | 'ble' | null; link_up?: boolean; site_time?: MountSiteTime | null } | null
  tracking: Tracking | null
  guiding: { state: 'off' | 'calibrating' | 'guiding'; calibration: Calibration | null; target: [number, number] | null; error_px: number | null; held?: boolean; dead_reckoning?: boolean }
  capture: {
    state: 'idle' | 'single' | 'sequence' | 'bursts' | 'flats' | 'flushing'
    phase: string; done: number; total: number | null; file: string | null; last_file: string | null
    burst: number | null; bursts_total: number | null; next_in_s: number | null
    dropped?: number; paused?: boolean; paused_reason?: string | null; stopped_early?: string | null; frames_lost?: string | null; eta_s?: number | null; direct_io?: { wanted: boolean; active: boolean; problem: string | null }; buffer_queued: number; buffer_capacity: number; buffer_mb: number; ram_available_mb: number | null; disk_mb_s: number
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

/** Failures of things the user did (or that should have worked) are shown in the status bar until dismissed; nothing fails quietly. */
let uiError: string | null = null
let uiErrorAt = 0
const uiErrorListeners = new Set<() => void>()
export function reportError(e: unknown, what: string) {
  const msg = `${what} failed: ${e instanceof Error ? e.message : String(e)}`
  if (msg === uiError && Date.now() - uiErrorAt < 30_000) return
  uiError = msg
  uiErrorAt = Date.now()
  uiErrorListeners.forEach((l) => l())
}
export function useUiError(): [string | null, () => void] {
  const [, force] = useState(0)
  useEffect(() => { const l = () => force((n) => n + 1); uiErrorListeners.add(l); return () => { uiErrorListeners.delete(l) } }, [])
  return [uiError, () => { uiError = null; uiErrorListeners.forEach((l) => l()) }]
}

export async function api<T = unknown>(method: 'GET' | 'POST' | 'PUT' | 'DELETE', path: string, body?: unknown): Promise<T> {
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
