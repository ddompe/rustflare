import { ClockPlace, MountSiteTime, Status } from './api'

/** "3 min 12 s", "45 s", "2 h 5 min". */
export function spanWords(secs: number): string {
  const s = Math.round(Math.abs(secs))
  if (s < 60) return `${s} s`
  if (s < 3600) return s % 60 ? `${Math.floor(s / 60)} min ${s % 60} s` : `${s / 60} min`
  const m = Math.floor((s % 3600) / 60)
  return m ? `${Math.floor(s / 3600)} h ${m} min` : `${s / 3600} h`
}

export const placeText = (c: ClockPlace) => (c.latitude_deg == null || c.longitude_east_deg == null ? 'not reported' : `${Math.abs(c.latitude_deg).toFixed(3)}° ${c.latitude_deg >= 0 ? 'N' : 'S'}, ${Math.abs(c.longitude_east_deg).toFixed(3)}° ${c.longitude_east_deg >= 0 ? 'E' : 'W'}`)

const pad = (n: number) => String(n).padStart(2, '0')
const stamp = (d: Date) => `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())} ${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}:${pad(d.getUTCSeconds())}`
const zoneName = (off: number) => `UTC${off < 0 ? '-' : '+'}${Math.floor(Math.abs(off) / 60)}${Math.abs(off) % 60 ? `:${pad(Math.abs(off) % 60)}` : ''}`

/** The clock in the Pi's local time first (its own zone), UTC second: "2026-11-26 21:04:10 (UTC+1) · 20:04:10 UTC". */
export function timeText(c: ClockPlace, piOffsetMinutes?: number | null): string {
  if (c.unix_secs == null) return 'not reported'
  const off = piOffsetMinutes ?? c.utc_offset_minutes ?? null
  const utc = stamp(new Date(c.unix_secs * 1000))
  if (off == null) return `${utc} UTC`
  return `${stamp(new Date((c.unix_secs + off * 60) * 1000))} (${zoneName(off)}) · ${utc.slice(11)} UTC`
}

const km = (deg: number) => Math.round(deg * 111)

/** Exact words: what was compared and what could not be ("time matches · place not reported", "time differs by 3 min · place matches"). */
export function clockSummary(st: MountSiteTime | null | undefined): string {
  if (!st) return 'not available'
  const time = st.time_ok == null ? 'time not reported' : st.time_ok ? 'time matches' : `time differs by ${st.time_diff_s != null ? spanWords(st.time_diff_s) : 'some'}`
  const place = st.place_ok == null ? 'place not reported' : st.place_ok ? 'place matches' : `place differs by ${st.place_diff_deg != null ? `${km(st.place_diff_deg)} km` : 'some'}`
  return `${time} · ${place}`
}

/** Why the mount cannot be sent the Pi's site and time right now, or null. */
export function sendReason(status: Status): string | null {
  const m = status.mount
  if (!m) return 'No mount is connected.'
  if (!m.site_time) return 'This mount cannot report its site and time.'
  if (!status.site) return 'Save your site first: the Pi has nothing to send.'
  if (m.park === 'parked' || m.park === 'parking') return 'The mount is parked: unpark it first.'
  if (m.slewing || status.goto || status.flip) return 'The mount is moving: wait until it stops.'
  return null
}

/**
 * The difference worth telling about: only something that can be compared and differs. Go to always sends the site and time first, so
 * this is a note (never a warning); with the auto-send switch on the note says when they go out by themselves.
 */
export function clockNote(status: Status): string | null {
  const st = status.mount?.site_time
  if (!st || !(st.time_ok === false || st.place_ok === false)) return null
  const what = st.time_ok === false && st.place_ok === false ? 'time and place differ from the Pi' : st.time_ok === false ? `clock is ${st.time_diff_s != null ? spanWords(st.time_diff_s) : 'some time'} off` : `place is ${st.place_diff_deg != null ? `${km(st.place_diff_deg)} km` : 'some distance'} off`
  const fix = status.mount?.auto_sync_site_time && status.clock?.ntp_synchronized !== false && status.site
    ? 'The Pi sends the right ones when the mount connects, and Go to sends them too.'
    : 'Go to sends the Pi’s time and place first. Send them now if you also steer from the mount’s handset.'
  return `Mount ${what}. ${fix}`
}

/** The mount-clock difference in a few words, to lead a chip with: "Clock off 3 min", "Place off 120 km", "Clock and place off". */
export function clockShort(status: Status): string | null {
  const st = status.mount?.site_time
  if (!st || !(st.time_ok === false || st.place_ok === false)) return null
  if (st.time_ok === false && st.place_ok === false) return 'Clock and place off'
  if (st.time_ok === false) return `Clock off ${st.time_diff_s != null ? spanWords(st.time_diff_s) : 'some time'}`
  return `Place off ${st.place_diff_deg != null ? `${km(st.place_diff_deg)} km` : 'some distance'}`
}
