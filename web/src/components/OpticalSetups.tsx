import { useEffect, useState } from 'react'
import { api, Status } from '../api'
import { PROFILES, profileOf } from '../profile'
import { ErrorLine, Field, useAction } from './ui'

/** A saved optical train: the telescope with its reducer or Barlow, and the profile it is used for. */
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
const num = (s: string) => (s.trim() === '' ? null : parseFloat(s))
const fmt = (v: number | null | undefined, d: number, unit: string) => (v == null ? 'not known' : `${v.toFixed(d)} ${unit}`)

/** What an applied setup changed, one line each, only the lines that differ. */
function changes(a: Applied): string[] {
  const out: string[] = []
  const { before: b, after: c } = a
  if (b.profile !== c.profile) out.push(`Profile: ${profileLabel(b.profile)} → ${profileLabel(c.profile)}`)
  if (b.effective_focal_mm !== c.effective_focal_mm) out.push(`Effective focal length: ${fmt(b.effective_focal_mm, 0, 'mm')} → ${fmt(c.effective_focal_mm, 0, 'mm')}`)
  if (b.binning !== c.binning) out.push(`Binning: ${b.binning} → ${c.binning}`)
  if (b.scale_arcsec_per_px !== c.scale_arcsec_per_px) out.push(`Image scale: ${fmt(b.scale_arcsec_per_px, 2, '″/px')} → ${fmt(c.scale_arcsec_per_px, 2, '″/px')}`)
  if (b.expected_radius_px !== c.expected_radius_px) out.push(`Expected radius: ${fmt(b.expected_radius_px, 0, 'px')} → ${fmt(c.expected_radius_px, 0, 'px')}`)
  return out
}

export interface OpticsForm { focal: string; mult: string; pixel: string; bin: number; dirty: boolean }

/** The setup picker of the Optics block: choose, apply, save as, update, duplicate, delete, revert. */
export function OpticalSetups({ status, form, onApplied }: { status: Status | null; form: OpticsForm; onApplied: () => void }) {
  const info = status?.optics?.setup
  const active = info?.active ?? null
  const modified = !!info?.modified
  const [list, setList] = useState<OpticalSetup[]>([])
  const [pick, setPick] = useState('')
  const [name, setName] = useState('')
  const [trainProfile, setTrainProfile] = useState<string>(profileOf(status))
  const [aperture, setAperture] = useState('')
  const [notes, setNotes] = useState('')
  const [ask, setAsk] = useState(false)
  const [result, setResult] = useState<Applied | null>(null)
  const { err, busy, run } = useAction()

  const reload = async () => { const r = await api<SetupList>('GET', '/optics/setups'); setList(r.setups); return r }
  useEffect(() => { reload().catch(() => {}) }, [active, modified]) // eslint-disable-line react-hooks/exhaustive-deps
  // the form follows the setup in use: its profile, aperture and notes
  const cur = list.find((s) => s.name === active)
  useEffect(() => {
    setPick(active ?? '')
    if (cur) { setTrainProfile(cur.profile ?? ''); setAperture(cur.aperture_mm?.toString() ?? ''); setNotes(cur.notes ?? '') }
  }, [active, cur?.name]) // eslint-disable-line react-hooks/exhaustive-deps

  const picked = list.find((s) => s.name === pick)
  const trimmed = name.trim()
  const nameOk = trimmed.length >= 1 && trimmed.length <= 40
  const effective = (num(form.focal) ?? 0) * (num(form.mult) ?? 1)
  const ap = num(aperture)
  const fratio = ap && effective ? effective / ap : null
  const body = () => ({
    profile: trainProfile || null,
    focal_length_mm: num(form.focal), focal_multiplier: num(form.mult) ?? 1, pixel_size_um: num(form.pixel), binning: form.bin,
    aperture_mm: ap, notes: notes.trim() || null,
  })
  /** What is typed in the optics fields is put in use first, so what is saved is what the engine has. */
  const useTyped = async () => {
    if (form.dirty) await api('PUT', '/optics', { focal_length_mm: num(form.focal), focal_multiplier: num(form.mult) ?? 1, pixel_size_um: num(form.pixel), binning: form.bin })
  }
  const save = (n: string) => run(async () => {
    await useTyped()
    await api('PUT', url(n), body())
    // the setup just saved is the one in use, with the profile chosen for it
    const r = await api<Applied>('POST', `${url(n)}/apply`)
    onApplied(); setName(''); setResult(r)
    await reload()
  })
  const apply = (n: string) => run(async () => {
    const r = await api<Applied>('POST', `${url(n)}/apply`)
    setResult(r); onApplied(); await reload()
  })
  const duplicate = () => run(async () => {
    if (!picked) return
    let n = `${picked.name} copy`.slice(0, 40)
    for (let i = 2; list.some((s) => s.name === n); i++) n = `${picked.name.slice(0, 34)} copy ${i}`
    await api('PUT', url(n), { ...picked, name: n, activate: false })
    await reload(); setPick(n); setResult(null)
  })
  const remove = (n: string) => run(async () => {
    await api('DELETE', url(n))
    setAsk(false); setResult(null); await reload(); setPick('')
  })

  const hint = list.length === 0
    ? 'None yet. Set the optics below, then give them a name with Save as. Each setup remembers the profile it is used for.'
    : picked
      ? [profileLabel(picked.profile) || 'keeps the current profile', picked.focal_length_mm ? `${picked.focal_length_mm} mm × ${picked.focal_multiplier}` : null, picked.aperture_mm && picked.focal_length_mm ? `f/${((picked.focal_length_mm * picked.focal_multiplier) / picked.aperture_mm).toFixed(1)}` : null, picked.notes].filter(Boolean).join(' · ')
      : undefined
  const edited = !!active && (modified || form.dirty || trainProfile !== (cur?.profile ?? '') || aperture !== (cur?.aperture_mm?.toString() ?? '') || notes !== (cur?.notes ?? ''))

  return (
    <div className="setups" role="group" aria-label="Optical setups">
      <div className="setup-head">
        <b>Optical setup</b>
        {active && <span className="setup-name">{active}</span>}
        {active && modified && <span className="setup-badge" role="status">modified</span>}
        {!active && <span className="hint-text">none in use</span>}
      </div>
      {active && modified && (
        <div className="btnrow tight">
          <p className="hint-text grow">The optics or the profile were changed after “{active}” was saved. They are not saved over it unless you press Update.</p>
          <button type="button" disabled={busy} onClick={() => apply(active)}>Revert</button>
        </div>
      )}
      <Field label="Saved setups" hint={hint}>
        {(id) => (
          <select id={id} value={pick} onChange={(e) => { setPick(e.target.value); setAsk(false) }}>
            <option value="">{list.length ? 'choose a setup' : 'no saved setups'}</option>
            {list.map((s) => <option key={s.name} value={s.name}>{s.name}{s.name === active ? ' (in use)' : ''}</option>)}
          </select>
        )}
      </Field>
      <div className="btnrow tight wrap">
        <button type="button" className="primary grow" disabled={busy || !picked || (picked.name === active && !modified)} onClick={() => apply(pick)}>Apply{picked ? ` “${picked.name}”` : ''}</button>
        <button type="button" disabled={busy || !picked} onClick={duplicate}>Duplicate</button>
        <button type="button" className="danger-outline" disabled={busy || !picked} onClick={() => setAsk(true)}>Delete…</button>
      </div>
      {ask && picked && (
        <div className="confirm" role="alertdialog" aria-label="Delete the setup">
          <p>Delete the setup “{picked.name}”?</p>
          <div className="hint-text">Forgotten: its name, focal length, Barlow or reducer factor, pixel size, binning, profile, aperture, notes{picked.flat || picked.dark ? ' and the flat and dark chosen for it' : ''}. The optics and the profile in use now stay as they are, and recordings and master files are not touched.</div>
          <div className="btnrow">
            <button type="button" onClick={() => setAsk(false)}>Keep it</button>
            <button type="button" className="danger grow" disabled={busy} onClick={() => remove(picked.name)}>Delete setup</button>
          </div>
        </div>
      )}
      {result && (
        <div className="setup-result" role="status" aria-label="What applying changed">
          <b>“{result.applied}” applied.</b>
          {changes(result).length ? <ul>{changes(result).map((l) => <li key={l}>{l}</li>)}</ul> : <span className="hint-text"> Nothing was different.</span>}
          <div className="hint-text">Nothing was moved and nothing was started.</div>
          {result.not_restored.map((n) => <div key={n.what} className="warn">The {n.what} “{n.name}” could not be restored: {n.reason}.</div>)}
        </div>
      )}

      <h4 className="setup-sub">Save these optics</h4>
      <Field label="Profile of this train" hint="Applying the setup switches to this profile. “Keep the current profile” leaves it alone.">
        {(id) => (
          <select id={id} value={trainProfile} onChange={(e) => setTrainProfile(e.target.value)}>
            <option value="">Keep the current profile</option>
            {PROFILES.map((p) => <option key={p.id} value={p.id}>{p.label}</option>)}
          </select>
        )}
      </Field>
      <div className="grid2">
        <Field label="Aperture (mm)" hint={fratio ? <span className="fratio">f/{fratio.toFixed(1)}</span> : 'optional, for the f-ratio'}>{(id) => <input id={id} inputMode="decimal" value={aperture} onChange={(e) => setAperture(e.target.value)} placeholder="mm" />}</Field>
        <Field label="Notes">{(id) => <input id={id} value={notes} maxLength={500} onChange={(e) => setNotes(e.target.value)} placeholder="optional" />}</Field>
      </div>
      {edited && <button type="button" className="primary wide" disabled={busy} onClick={() => save(active!)}>Update “{active}”</button>}
      <Field label="Name for a new setup">{(id) => <input id={id} value={name} maxLength={60} onChange={(e) => setName(e.target.value)} placeholder="e.g. Lunt + Barlow" />}</Field>
      <button type="button" className="wide" disabled={busy || !nameOk} onClick={() => save(trimmed)}>Save as…</button>
      {name && !nameOk && <div className="hint-text" role="note">The name needs 1 to 40 characters.</div>}
      <ErrorLine error={err} />
    </div>
  )
}
