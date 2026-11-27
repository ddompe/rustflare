import { ReactNode, useEffect, useId, useRef, useState } from 'react'
import { api, Status } from '../api'
import { noun, PROFILES, profileOf, ProfileId } from '../profile'
import { ErrorLine, ToneIcon, useAction } from './ui'
import { Modal } from './Modal'

/** A saved optical setup: the telescope with its reducer or Barlow, and the profile it is used for. */
export interface OpticalSetup {
  name: string
  profile: string | null
  focal_length_mm: number | null
  focal_multiplier: number
  pixel_size_um: number | null
  binning: number
  aperture_mm: number | null
  notes: string | null
  flat: string | null
  dark: string | null
  dark_exposure_us?: number | null
  dark_gain?: number | null
}
interface SetupList { active: string | null; modified: boolean; setups: OpticalSetup[] }
interface Snapshot { profile: string; focal_length_mm: number | null; effective_focal_mm: number | null; scale_arcsec_per_px: number | null; expected_radius_px: number | null; binning: number }
interface Applied { applied: string; before: Snapshot; after: Snapshot; not_restored: { what: string; name: string; reason: string }[] }

export const profileLabel = (id: string | null | undefined) => PROFILES.find((p) => p.id === id)?.label ?? id ?? ''

/** The name of the setup in use with its profile, as the camera tab and the profile picker show it: "Lunt + Barlow · Sun, H-alpha". */
export function setupLine(status: Status | null | undefined): string | null {
  const s = status?.optics?.setup
  if (!s?.active) return null
  return `${s.active}${s.modified ? ' (modified)' : ''} · ${profileLabel(profileOf(status))}`
}

const url = (name: string) => `/optics/setups/${encodeURIComponent(name)}`
/** Blank is none; anything that is not a number is NaN (and refused by the form). */
const num = (s: string): number | null => (s.trim() === '' ? null : Number(s))
const r0 = (n: number) => String(Math.round(n))
const r1 = (n: number) => String(Number(n.toFixed(1)))
const r2 = (n: number) => n.toFixed(2)
const isNum = (n: number | null): n is number => n != null && Number.isFinite(n)
const SCALE = 206.265 // arcseconds per pixel = 206.265 × pixel (µm) × binning / focal length (mm)

/** "800 mm (400 × 2) · f/16", the optics of a setup in one run. */
function opticsText(focal: number | null, mult: number, aperture: number | null): string {
  if (!isNum(focal)) return 'no focal length'
  const eff = focal * mult
  return `${r0(eff)} mm${mult !== 1 ? ` (${r0(focal)} × ${mult})` : ''}${isNum(aperture) && aperture > 0 ? ` · f/${r1(eff / aperture)}` : ''}`
}

/** What is in use, read only: "800 mm (400 × 2) · f/16 · 1.60″/px · expected Sun radius 540 px · Sun, H-alpha". */
export function opticsSummary(status: Status | null, aperture: number | null): string {
  const o = status?.optics
  if (!o) return ''
  const p = profileOf(status)
  const parts = [opticsText(o.focal_length_mm, o.focal_multiplier, aperture)]
  if (o.scale_arcsec_per_px) parts.push(`${r2(o.scale_arcsec_per_px)}″/px`)
  if (o.expected_radius_px) parts.push(`expected ${noun(p)} radius ${r0(o.expected_radius_px)} px`)
  parts.push(profileLabel(p))
  return parts.join(' · ')
}

/** Does what is in use differ from this setup? The profile counts only when the setup has one (the server's rule). */
function differs(s: OpticalSetup, status: Status | null): boolean {
  const o = status?.optics
  if (!o) return true
  const same = (a: number | null, b: number | null) => (a == null || b == null ? a === b : Math.abs(a - b) < 1e-6)
  return !same(s.focal_length_mm, o.focal_length_mm) || !same(s.focal_multiplier, o.focal_multiplier) || !same(s.pixel_size_um, o.pixel_size_override_um) || s.binning !== o.binning || (!!s.profile && s.profile !== profileOf(status))
}

const useList = (reloadOn: unknown[]) => {
  const [list, setList] = useState<OpticalSetup[]>([])
  const reload = async () => { const r = await api<SetupList>('GET', '/optics/setups'); setList(r.setups); return r.setups }
  useEffect(() => { reload().catch(() => {}) }, reloadOn) // eslint-disable-line react-hooks/exhaustive-deps
  return { list, reload }
}
const RECORDING_WHY = 'Off while a recording runs: the optical setup and the profile are written into the files being recorded.'

/** The page: one dropdown of the saved setups (choosing one applies it), one Manage button, one read-only line of what is in use. */
export function OpticalSetupPicker({ status, openManager, modalOpen }: { status: Status | null; openManager: () => void; modalOpen: boolean }) {
  const info = status?.optics?.setup
  const active = info?.active ?? null
  const modified = !!info?.modified
  const recording = !!status && status.capture.state !== 'idle'
  const { list, reload } = useList([active, modified, modalOpen])
  const [line, setLine] = useState<{ text: string; warn: string[] } | null>(null)
  const { err, busy, run } = useAction()
  const id = useId()
  useEffect(() => { if (modalOpen) setLine(null) }, [modalOpen])

  const match = active ? null : list.find((s) => !differs(s, status))?.name ?? null
  const value = active ?? match ?? ''
  const names = list.map((s) => s.name)
  if (active && !names.includes(active)) names.push(active)
  const apply = (name: string, verb: string) => run(async () => {
    const r = await api<Applied>('POST', `${url(name)}/apply`)
    const a = r.after
    setLine({
      text: `“${r.applied}” ${verb}: ${isNum(a.effective_focal_mm) ? `${r0(a.effective_focal_mm)} mm` : 'no focal length'} · ${a.scale_arcsec_per_px ? `${r2(a.scale_arcsec_per_px)}″/px · ` : ''}${profileLabel(a.profile)}`,
      warn: r.not_restored.map((n) => `The ${n.what} “${n.name}” could not be restored: ${n.reason}.`),
    })
    await reload()
  })
  const aperture = list.find((s) => s.name === value)?.aperture_mm ?? null

  return (
    <div className="setups">
      <div className="field">
        <label htmlFor={id}>Optical setup</label>
        <select id={id} value={value} disabled={recording || busy} aria-describedby={recording ? `${id}-why` : undefined} onChange={(e) => { if (e.target.value) apply(e.target.value, 'applied') }}>
          {value === '' && <option value="">Custom (not saved)</option>}
          {names.map((n) => <option key={n} value={n}>{n}{n === active && modified ? ' (modified)' : ''}</option>)}
        </select>
        {recording && <p className="gate-why hint-text" id={`${id}-why`} role="note">{RECORDING_WHY}</p>}
      </div>
      {active && modified && (
        <p className="hint-text setup-modified">
          <ToneIcon tone="warn" size={12} /> Changed since “{active}” was saved.{' '}
          <button type="button" className="quiet" disabled={busy || recording} aria-describedby={recording ? `${id}-why` : undefined} onClick={() => apply(active, 'restored')}>Revert</button>
        </p>
      )}
      {line && (
        <div className="setup-result" role="status">
          <span>{line.text}</span>
          {line.warn.map((w) => <div key={w} className="warn">{w}</div>)}
        </div>
      )}
      <ErrorLine error={err} />
      <button type="button" className="wide" onClick={openManager}>Manage…</button>
      <p className="setup-summary" aria-label="In use">{opticsSummary(status, aperture)}</p>
    </div>
  )
}

/** A form row with its error under it, joined to the control for screen readers. */
function Row({ k, label, error, hint, children }: { k?: string; label: string; error?: string | null; hint?: ReactNode; children: (id: string, describedBy: string | undefined, invalid: boolean) => ReactNode }) {
  const uid = useId()
  const id = k ? `setup-f-${k}` : uid
  const eid = `${id}-e`
  return (
    <div className="field">
      <label htmlFor={id}>{label}</label>
      {children(id, error ? eid : undefined, !!error)}
      {error && <div className="field-err" id={eid}><ToneIcon tone="bad" size={12} /> {error}</div>}
      {hint && <div className="hint-text">{hint}</div>}
    </div>
  )
}

const baseName = (p: string | null | undefined) => (p ? p.split('/').pop() ?? p : null)
const BINNING = [1, 2, 3, 4]

/** The one form, for New and Edit. */
function SetupForm({ status, list, editing, onDone, onCancel }: { status: Status | null; list: OpticalSetup[]; editing: OpticalSetup | null; onDone: (message: string) => void; onCancel: () => void }) {
  const o = status?.optics
  const active = o?.setup?.active ?? null
  const recording = !!status && status.capture.state !== 'idle'
  const isActive = !!editing && editing.name === active
  const activeSetup = list.find((s) => s.name === active)
  const [name, setName] = useState(editing?.name ?? '')
  const [focal, setFocal] = useState((editing ? editing.focal_length_mm : o?.focal_length_mm)?.toString() ?? '')
  const [mult, setMult] = useState(String(editing ? editing.focal_multiplier : o?.focal_multiplier ?? 1))
  const [pixel, setPixel] = useState((editing ? editing.pixel_size_um : o?.pixel_size_override_um)?.toString() ?? '')
  const [bin, setBin] = useState(editing ? editing.binning : o?.binning ?? 1)
  const [aperture, setAperture] = useState((editing ? editing.aperture_mm : activeSetup?.aperture_mm)?.toString() ?? '')
  const [profile, setProfile] = useState<string>(editing ? editing.profile ?? '' : profileOf(status))
  const [notes, setNotes] = useState(editing?.notes ?? '')
  // New: store the flat and dark in use (on). Edit: keep the stored ones, or replace them with those in use.
  const [useCurrent, setUseCurrent] = useState(!editing)
  const [tried, setTried] = useState(false)
  const { err, busy, run } = useAction()
  const head = useRef<HTMLHeadingElement>(null)
  useEffect(() => { head.current?.focus() }, [])

  const curFlat = status?.flats?.name ?? null
  const curDark = status?.darks?.name ?? null
  const flat = editing && !useCurrent ? editing.flat : curFlat
  const dark = editing && !useCurrent ? editing.dark : curDark
  const nm = name.trim()
  const f = num(focal), m = num(mult), px = num(pixel), ap = num(aperture)
  const errors: Record<string, string | null> = {
    name: nm === '' ? 'Give the setup a name.' : [...nm].length > 40 ? 'The name is at most 40 characters.' : nm.includes('/') ? 'The name cannot contain a slash.' : /[\u0000-\u001f\u007f]/.test(nm) ? 'The name cannot contain control characters.' : list.some((s) => s.name === nm && s.name !== editing?.name) ? `There is already an optical setup called “${nm}”.` : null,
    focal: f != null && (!Number.isFinite(f) || f < 10 || f > 20000) ? 'The focal length is 10 to 20000 mm.' : null,
    mult: m == null || !Number.isFinite(m) || m < 0.2 || m > 10 ? 'The factor is 0.2 to 10 (2 for a 2× Barlow, 0.63 for a reducer, 1 for none).' : null,
    pixel: px != null && (!Number.isFinite(px) || px < 0.5 || px > 50) ? 'The pixel size is 0.5 to 50 µm.' : null,
    aperture: ap != null && (!Number.isFinite(ap) || ap < 5 || ap > 5000) ? 'The aperture is 5 to 5000 mm.' : null,
    notes: notes.trim().length > 500 ? 'Notes are at most 500 characters.' : null,
  }
  // an error shows when it is certain (the field has something in it) or once Save was pressed
  const touched: Record<string, boolean> = { name: nm !== '', focal: focal.trim() !== '', mult: true, pixel: pixel.trim() !== '', aperture: aperture.trim() !== '', notes: true }
  const shown = (k: string) => (tried || touched[k] ? errors[k] : null)
  const firstBad = Object.keys(errors).find((k) => errors[k])

  const effective = isNum(f) && isNum(m) ? f * m : null
  const camPx = o?.pixel_size_source === 'camera' ? o.pixel_size_um : null
  const pxUse = isNum(px) ? px : camPx
  const scale = effective && isNum(pxUse) ? (SCALE * pxUse * bin) / effective : null
  // the radius belongs to the profile: only what is known (the profile in use, or the same kind of target) is computed
  const cur = profileOf(status)
  const sameTarget = profile === '' || profile === cur || (profile.startsWith('sun') && cur.startsWith('sun'))
  const radiusArcsec = o ? o.target_radius_arcsec ?? o.solar_radius_arcsec : null
  const radius = scale && sameTarget && radiusArcsec ? radiusArcsec / scale : null
  const fratio = effective && isNum(ap) && ap > 0 ? effective / ap : null

  const submit = (use: boolean) => {
    setTried(true)
    if (firstBad) { document.getElementById(`setup-f-${firstBad}`)?.focus(); return }
    const body: Record<string, unknown> = {
      profile: profile || null, focal_length_mm: f, focal_multiplier: m, pixel_size_um: px, binning: bin, aperture_mm: ap, notes: notes.trim() || null,
    }
    if (editing) {
      if (nm !== editing.name) body.rename_to = nm
      if (!useCurrent) Object.assign(body, { flat: editing.flat, dark: editing.dark, dark_exposure_us: editing.dark_exposure_us ?? null, dark_gain: editing.dark_gain ?? null })
      if (!isActive) body.activate = false
    } else if (!useCurrent) {
      Object.assign(body, { flat: null, dark: null, dark_exposure_us: null, dark_gain: null })
    }
    // the flat and dark in use are stored when the keys are left out: the server fills them from what is in use
    run(async () => {
      await api('PUT', url(editing ? editing.name : nm), body)
      const applies = use || isActive
      if (applies && !recording) await api('POST', `${url(nm)}/apply`)
      onDone(applies && recording ? `Saved “${nm}”. Not applied: a recording is running.` : applies ? `Saved “${nm}” and in use.` : `Saved “${nm}”.`)
    })
  }
  const bins = BINNING.includes(bin) ? BINNING : [...BINNING, bin].sort((a, b) => a - b)

  return (
    <form className="modal-view" noValidate onSubmit={(e) => { e.preventDefault(); submit(false) }}>
      <div className="modal-body">
        <button type="button" className="quiet back" onClick={onCancel}>‹ Back to the list</button>
        <h3 className="modal-sub" tabIndex={-1} ref={head}>{editing ? `Edit “${editing.name}”` : 'New optical setup'}</h3>
        {!editing && <p className="hint-text">Starts from the optics in use. Choose a name and save it.</p>}
        <Row k="name" label="Name" error={shown('name')}>{(id, d, bad) => <input id={id} aria-describedby={d} aria-invalid={bad} autoComplete="off" value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. Lunt + Barlow" />}</Row>
        <div className="grid2">
          <Row k="focal" label="Focal length (mm)" error={shown('focal')}>{(id, d, bad) => <input id={id} aria-describedby={d} aria-invalid={bad} inputMode="decimal" value={focal} onChange={(e) => setFocal(e.target.value)} placeholder="telescope" />}</Row>
          <Row k="mult" label="Barlow or reducer (×)" error={shown('mult')}>
            {(id, d, bad) => (
              <>
                <input id={id} aria-describedby={d} aria-invalid={bad} inputMode="decimal" list="barlow-presets" value={mult} onChange={(e) => setMult(e.target.value)} />
                <datalist id="barlow-presets">{[0.5, 0.63, 0.8, 1, 1.25, 1.5, 2, 2.5, 3, 4, 5].map((x) => <option key={x} value={x} />)}</datalist>
              </>
            )}
          </Row>
          <Row k="pixel" label="Pixel size override (µm)" error={shown('pixel')} hint={isNum(camPx) && !isNum(px) ? `blank: the camera’s ${camPx} µm` : 'optional'}>{(id, d, bad) => <input id={id} aria-describedby={d} aria-invalid={bad} inputMode="decimal" value={pixel} onChange={(e) => setPixel(e.target.value)} placeholder="µm" />}</Row>
          <Row label="Binning">{(id) => <select id={id} value={bin} onChange={(e) => setBin(parseInt(e.target.value))}>{bins.map((b) => <option key={b} value={b}>{b}</option>)}</select>}</Row>
        </div>
        <Row k="aperture" label="Aperture (mm)" error={shown('aperture')} hint={fratio ? <span className="fratio">f/{r1(fratio)}</span> : 'optional, for the f-ratio'}>{(id, d, bad) => <input id={id} aria-describedby={d} aria-invalid={bad} inputMode="decimal" value={aperture} onChange={(e) => setAperture(e.target.value)} placeholder="mm" />}</Row>
        <Row label="Profile for this setup" hint="Using the setup switches to this profile.">
          {(id) => (
            <select id={id} value={profile} onChange={(e) => setProfile(e.target.value)}>
              <option value="">Keep the current profile</option>
              {PROFILES.map((p) => <option key={p.id} value={p.id}>{p.label}</option>)}
            </select>
          )}
        </Row>
        <Row k="notes" label="Notes" error={shown('notes')}>{(id, d, bad) => <input id={id} aria-describedby={d} aria-invalid={bad} value={notes} onChange={(e) => setNotes(e.target.value)} placeholder="optional" />}</Row>
        <p className="setup-calc" aria-live="polite">
          {effective ? `${r0(effective)} mm effective` : 'No focal length yet'}{scale ? ` · ${r2(scale)}″/px` : ' · scale not known (needs a focal length and a pixel size)'}{radius ? ` · expected ${noun(profile ? (profile as ProfileId) : cur)} radius ${r0(radius)} px` : ''}
        </p>
        <div className="setup-stored">
          <p>Flat: <b>{baseName(flat) ?? 'none'}</b></p>
          <p>Dark: <b>{baseName(dark) ?? 'none'}</b></p>
          <label className="check">
            <input type="checkbox" checked={useCurrent} onChange={(e) => setUseCurrent(e.target.checked)} />
            <span>{editing ? 'Replace the stored flat and dark with the current ones' : 'Include the current flat and dark'}</span>
          </label>
          {editing && !useCurrent && <p className="hint-text">The stored ones are kept; the current ones are {baseName(curFlat) ?? 'no flat'} and {baseName(curDark) ?? 'no dark'}.</p>}
        </div>
      </div>
      <div className="modal-foot">
        <ErrorLine error={err} />
        {tried && firstBad && <div className="err" role="alert">Fix the marked fields before saving.</div>}
        <div className="btnrow tight wrap">
          <button type="button" className="grow" onClick={onCancel}>Cancel</button>
          {!editing && <button type="button" className="grow" disabled={busy || recording} aria-describedby={recording ? 'setup-use-why' : undefined} onClick={() => submit(true)}>Save and use</button>}
          <button type="submit" className="primary grow" disabled={busy}>Save</button>
        </div>
        {recording && !editing && <p className="gate-why hint-text" id="setup-use-why" role="note">Save and use is off while a recording runs: Save keeps it for later.</p>}
        {recording && isActive && <p className="hint-text">A recording runs: Save keeps the new values, they are put to use afterwards.</p>}
      </div>
    </form>
  )
}

/** Manage: the list of saved setups with Edit, Duplicate and Delete, and the one form for New and Edit. */
export function OpticalSetupsModal({ status, onClose }: { status: Status | null; onClose: () => void }) {
  const info = status?.optics?.setup
  const active = info?.active ?? null
  const modified = !!info?.modified
  const [view, setView] = useState<{ form: true; editing: string | null } | null>(null)
  const { list, reload } = useList([active, modified])
  const [note, setNote] = useState<string | null>(null)
  const [ask, setAsk] = useState<string | null>(null)
  const [focusId, setFocusId] = useState<string | null>(null)
  const { err, busy, run } = useAction()
  useEffect(() => { if (focusId) { document.getElementById(focusId)?.focus(); setFocusId(null) } }, [focusId, view, ask])

  const duplicate = (s: OpticalSetup) => run(async () => {
    let n = `${s.name} copy`.slice(0, 40)
    for (let i = 2; list.some((x) => x.name === n); i++) n = `${s.name.slice(0, 34)} copy ${i}`
    await api('PUT', url(n), { ...s, name: n, activate: false })
    await reload()
    setNote(`Duplicated as “${n}”.`)
    setFocusId(`setup-edit-${n}`)
  })
  const remove = (n: string) => run(async () => {
    await api('DELETE', url(n))
    setAsk(null); setNote(`Deleted “${n}”.`)
    await reload()
    setFocusId('setup-new')
  })
  const editing = view?.editing ? list.find((s) => s.name === view.editing) ?? null : null

  return (
    <Modal title="Optical setups" onClose={onClose}>
      {view && (view.editing == null || editing) ? (
        <SetupForm
          key={view.editing ?? '*new'} status={status} list={list} editing={editing}
          onCancel={() => { setView(null); setFocusId(view.editing ? `setup-edit-${view.editing}` : 'setup-new') }}
          onDone={async (m) => { await reload(); setView(null); setNote(m); setFocusId('setup-new') }}
        />
      ) : (
        <div className="modal-view">
          <div className="modal-body">
            {note && <p className="setup-note" role="status">{note}</p>}
            <ErrorLine error={err} />
            {list.length === 0 && <p className="hint-text">No saved optical setups yet. “New setup…” starts from the optics in use.</p>}
            <ul className="setup-list">
              {list.map((s) => {
                const aperture = s.aperture_mm
                return (
                  <li key={s.name} className="setup-row">
                    <div className="setup-row-head">
                      <b className="setup-name">{s.name}</b>
                      {s.name === active && <span className="tag ok"><ToneIcon tone="ok" size={12} /> In use{modified ? ', modified' : ''}</span>}
                    </div>
                    <div className="hint-text">{opticsText(s.focal_length_mm, s.focal_multiplier, aperture)} · {profileLabel(s.profile) || 'keeps the current profile'}</div>
                    {ask === s.name ? (
                      <div className="confirm" role="group" aria-label={`Delete “${s.name}”?`}>
                        <p>Delete “{s.name}”? Its values, profile, flat and dark names are forgotten.</p>
                        <div className="btnrow tight">
                          <button type="button" className="grow" id={`setup-cancel-${s.name}`} autoFocus onClick={() => { setAsk(null); setFocusId(`setup-del-${s.name}`) }}>Cancel</button>
                          <button type="button" className="danger grow" disabled={busy} onClick={() => remove(s.name)}>Delete</button>
                        </div>
                      </div>
                    ) : (
                      <div className="btnrow tight wrap">
                        <button type="button" className="small" id={`setup-edit-${s.name}`} aria-label={`Edit “${s.name}”`} onClick={() => { setNote(null); setView({ form: true, editing: s.name }) }}>Edit</button>
                        <button type="button" className="small" aria-label={`Duplicate “${s.name}”`} disabled={busy} onClick={() => duplicate(s)}>Duplicate</button>
                        <button type="button" className="small danger-outline" id={`setup-del-${s.name}`} aria-label={`Delete “${s.name}”`} onClick={() => setAsk(s.name)}>Delete</button>
                      </div>
                    )}
                  </li>
                )
              })}
            </ul>
          </div>
          <div className="modal-foot">
            <button type="button" className="primary wide" id="setup-new" onClick={() => { setNote(null); setAsk(null); setView({ form: true, editing: null }) }}>New setup…</button>
          </div>
        </div>
      )}
    </Modal>
  )
}
