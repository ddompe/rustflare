import { useEffect, useState } from 'react'
import { api, Status } from '../api'
import { PROFILES, profileOf } from '../profile'
import { ErrorLine, Field, ToneIcon, useAction } from './ui'

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

/** What saving over the setup in use would change, as "replaces 400 mm × 2 with 400 mm × 1.5" (empty when nothing differs). */
export function updateDiff(cur: OpticalSetup | undefined, now: { focal: string; mult: string; pixel: string; bin: number; profile: string; aperture: string; notes: string }): string {
  if (!cur) return ''
  const olds: string[] = [], news: string[] = []
  const fl = num(now.focal), ml = num(now.mult) ?? 1, px = num(now.pixel), ap = num(now.aperture)
  const fm = (f: number | null, m: number) => `${f ?? 'no focal length'}${f != null ? ' mm' : ''} × ${m}`
  if (cur.focal_length_mm !== fl || cur.focal_multiplier !== ml) { olds.push(fm(cur.focal_length_mm, cur.focal_multiplier)); news.push(fm(fl, ml)) }
  if (cur.pixel_size_um !== px) { olds.push(`pixel ${cur.pixel_size_um ?? 'from the camera'}${cur.pixel_size_um != null ? ' µm' : ''}`); news.push(`pixel ${px ?? 'from the camera'}${px != null ? ' µm' : ''}`) }
  if (cur.binning !== now.bin) { olds.push(`binning ${cur.binning}`); news.push(`binning ${now.bin}`) }
  if ((cur.profile ?? '') !== now.profile) { olds.push(profileLabel(cur.profile) || 'keeps the profile'); news.push(profileLabel(now.profile) || 'keeps the profile') }
  if (cur.aperture_mm !== ap) { olds.push(`aperture ${cur.aperture_mm ?? 'none'}${cur.aperture_mm != null ? ' mm' : ''}`); news.push(`aperture ${ap ?? 'none'}${ap != null ? ' mm' : ''}`) }
  if ((cur.notes ?? '') !== now.notes.trim()) { olds.push('old notes'); news.push('new notes') }
  return olds.length ? `replaces ${olds.join(', ')} with ${news.join(', ')}` : ''
}

/** The saved optical setups: choose, apply, duplicate, delete, revert; then one Save group (update the one in use with a diff and a confirm, or save as new). */
export function OpticalSetups({ status, form, onApplied }: { status: Status | null; form: OpticsForm; onApplied: () => void }) {
  const info = status?.optics?.setup
  const active = info?.active ?? null
  const modified = !!info?.modified
  const [list, setList] = useState<OpticalSetup[]>([])
  const [pick, setPick] = useState('')
  const [name, setName] = useState('')
  const [setupProfile, setSetupProfile] = useState<string>(profileOf(status))
  const [aperture, setAperture] = useState('')
  const [notes, setNotes] = useState('')
  // the optics and the profile are written into the files being recorded: applying waits for the end of the recording
  const recording = !!status && status.capture.state !== 'idle'
  const [ask, setAsk] = useState(false)
  const [askUpdate, setAskUpdate] = useState(false)
  const [result, setResult] = useState<Applied | null>(null)
  const { err, busy, run } = useAction()

  const reload = async () => { const r = await api<SetupList>('GET', '/optics/setups'); setList(r.setups); return r }
  useEffect(() => { reload().catch(() => {}) }, [active, modified]) // eslint-disable-line react-hooks/exhaustive-deps
  // the form follows the setup in use: its profile, aperture and notes
  const cur = list.find((s) => s.name === active)
  useEffect(() => {
    setPick(active ?? '')
    setAskUpdate(false)
    if (cur) { setSetupProfile(cur.profile ?? ''); setAperture(cur.aperture_mm?.toString() ?? ''); setNotes(cur.notes ?? '') }
  }, [active, cur?.name]) // eslint-disable-line react-hooks/exhaustive-deps

  const picked = list.find((s) => s.name === pick)
  const trimmed = name.trim()
  const nameOk = trimmed.length >= 1 && trimmed.length <= 40
  const effective = (num(form.focal) ?? 0) * (num(form.mult) ?? 1)
  const ap = num(aperture)
  const fratio = ap && effective ? effective / ap : null
  const body = () => ({
    profile: setupProfile || null,
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
    onApplied(); setName(''); setAskUpdate(false); setResult(r)
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
    ? 'None yet. Set the values above, then give them a name with Save as new.'
    : picked
      ? [profileLabel(picked.profile) || 'keeps the current profile', picked.focal_length_mm ? `${picked.focal_length_mm} mm × ${picked.focal_multiplier}` : null, picked.aperture_mm && picked.focal_length_mm ? `f/${((picked.focal_length_mm * picked.focal_multiplier) / picked.aperture_mm).toFixed(1)}` : null, picked.notes].filter(Boolean).join(' · ')
      : undefined
  const diff = updateDiff(cur, { focal: form.focal, mult: form.mult, pixel: form.pixel, bin: form.bin, profile: setupProfile, aperture, notes })
  const edited = !!active && (modified || form.dirty || diff !== '')
  const whyPick = !picked ? 'Choose an optical setup in the list first.' : null
  const whyApplyBtn = recording ? 'Applying is off while a recording runs: the optical setup and the profile are written into the files being recorded.' : !picked ? 'Choose an optical setup in the list first.' : picked.name === active && !modified ? `“${picked.name}” is already in use and unchanged.` : null
  const whyApply = recording ? 'Applying is off while a recording runs: the optical setup and the profile are written into the files being recorded.' : null

  return (
    <div className="setups" role="group" aria-label="Saved optical setups">
      <h2 className="subhead">Saved optical setups</h2>
      <div className="setup-head" aria-label={active ? `Optical setup, in use: ${active}` : 'Optical setup, none in use'} role="group">
        <b>In use:</b>
        {active && <span className="setup-name">{active}</span>}
        {active && modified && <span className="setup-badge" role="status"><ToneIcon tone="warn" size={12} /> modified</span>}
        {!active && <span className="hint-text">none</span>}
      </div>
      {active && modified && (
        <div className="btnrow tight">
          <p className="hint-text grow">The values or the profile were changed after “{active}” was saved. They are not saved over it unless you press Update.</p>
          <button type="button" disabled={busy || recording} aria-describedby={whyApply ? 'why-setup-recording' : undefined} onClick={() => apply(active)}>Revert</button>
          {whyApply && <p className="gate-why hint-text" id="why-setup-recording" role="note">{whyApply}</p>}
        </div>
      )}
      <Field label="Optical setup" hint={hint}>
        {(id) => (
          <select id={id} value={pick} onChange={(e) => { setPick(e.target.value); setAsk(false) }}>
            <option value="">{list.length ? 'choose an optical setup' : 'no saved optical setups'}</option>
            {list.map((s) => <option key={s.name} value={s.name}>{s.name}{s.name === active ? ' (in use)' : ''}</option>)}
          </select>
        )}
      </Field>
      <div className="btnrow tight wrap">
        <button type="button" className="primary grow" disabled={busy || recording || !picked || (picked.name === active && !modified)} aria-describedby={whyApplyBtn ? 'why-setup-apply' : undefined} onClick={() => apply(pick)}>Apply{picked ? ` “${picked.name}”` : ''}</button>
        <button type="button" disabled={busy || !picked} aria-describedby={whyPick ? 'why-setup-pick' : undefined} onClick={duplicate}>Duplicate</button>
        <button type="button" className="danger-outline" disabled={busy || !picked} aria-describedby={whyPick ? 'why-setup-pick' : undefined} onClick={() => setAsk(true)}>Delete…</button>
      </div>
      {whyApplyBtn && <p className="gate-why hint-text" id="why-setup-apply" role="note">{whyApplyBtn}</p>}
      {whyPick && !whyApplyBtn && <p className="gate-why hint-text" id="why-setup-pick" role="note">{whyPick}</p>}
      {whyPick && whyApplyBtn && <span className="sr-only" id="why-setup-pick">{whyPick}</span>}
      {ask && picked && (
        <div className="confirm" role="alertdialog" aria-label="Delete the optical setup">
          <p>Delete the optical setup “{picked.name}”?</p>
          <div className="hint-text">Forgotten: its name, focal length, Barlow or reducer factor, pixel size, binning, profile, aperture, notes{picked.flat || picked.dark ? ' and the flat and dark chosen for it' : ''}. The values and the profile in use now stay as they are, and recordings and master files are not touched.</div>
          <div className="btnrow">
            <button type="button" className="grow" onClick={() => setAsk(false)}>Cancel</button>
            <button type="button" className="danger grow" disabled={busy} onClick={() => remove(picked.name)}>Delete “{picked.name}”</button>
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

      <h2 className="subhead">Save</h2>
      <Field label="Profile to switch to when applied" hint="Applying the optical setup switches to this profile. “Keep the current profile” leaves it alone.">
        {(id) => (
          <select id={id} value={setupProfile} onChange={(e) => setSetupProfile(e.target.value)}>
            <option value="">Keep the current profile</option>
            {PROFILES.map((p) => <option key={p.id} value={p.id}>{p.label}</option>)}
          </select>
        )}
      </Field>
      <div className="grid2">
        <Field label="Aperture (mm)" hint={fratio ? <span className="fratio">f/{fratio.toFixed(1)}</span> : 'optional, for the f-ratio'}>{(id) => <input id={id} inputMode="decimal" value={aperture} onChange={(e) => setAperture(e.target.value)} placeholder="mm" />}</Field>
        <Field label="Notes">{(id) => <input id={id} value={notes} maxLength={500} onChange={(e) => setNotes(e.target.value)} placeholder="optional" />}</Field>
      </div>
      {edited && active && (
        <div className="setup-update">
          <p className="hint-text setup-diff" id="setup-diff">{diff ? <>Update “{active}”: {diff}.</> : <>“{active}” was changed after it was saved.</>}</p>
          {!askUpdate ? (
            <button type="button" className="primary wide" disabled={busy} aria-describedby="setup-diff" onClick={() => setAskUpdate(true)}>Update “{active}”…</button>
          ) : (
            <div className="confirm" role="alertdialog" aria-label={`Update the optical setup ${active}`}>
              <p>Replace the saved “{active}”?</p>
              <div className="hint-text">{diff ? `It ${diff}.` : 'It keeps the values now in use.'} The old values cannot be brought back.</div>
              <div className="btnrow">
                <button type="button" className="grow" onClick={() => setAskUpdate(false)}>Cancel</button>
                <button type="button" className="danger grow" disabled={busy} onClick={() => save(active)}>Update “{active}”</button>
              </div>
            </div>
          )}
        </div>
      )}
      <Field label="Name for a new optical setup">{(id) => <input id={id} value={name} maxLength={60} onChange={(e) => setName(e.target.value)} placeholder="e.g. Lunt + Barlow" />}</Field>
      <button type="button" className="wide" disabled={busy || !nameOk} aria-describedby={!nameOk ? 'why-setup-name' : undefined} onClick={() => save(trimmed)}>Save as new…</button>
      {!nameOk && <div className="hint-text gate-why" id="why-setup-name" role="note">{name ? 'The name needs 1 to 40 characters.' : 'Type a name above to save the values in use as a new optical setup.'}</div>}
      <ErrorLine error={err} />
    </div>
  )
}
