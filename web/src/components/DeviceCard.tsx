import { CameraCatalog, CameraLive, Datasheets, DeviceKind, MountCatalog, MountLive, deviceImage } from '../datasheet'

type Row = [string, string | number | undefined | null]
const has = (v: unknown) => v !== undefined && v !== null && v !== '' && !(Array.isArray(v) && v.length === 0)
const list = (v?: string | string[]) => (Array.isArray(v) ? v.join(', ') : v)
const yn = (v?: boolean) => (v === undefined ? undefined : v ? 'yes' : 'no')

function cameraRows(live: CameraLive = {}, cat: CameraCatalog | null | undefined): { title: string; facts: string; unit: Row[]; spec: Row[] } {
  const size = live.width && live.height ? `${live.width}×${live.height}` : undefined
  const facts = [size, has(live.pixel_um) ? `${live.pixel_um} µm` : undefined, has(live.bits) ? `${live.bits}-bit` : undefined, live.color === undefined ? undefined : live.color ? 'colour' : 'mono', live.cooled ? 'cooled' : undefined].filter(has).join(' · ')
  return {
    title: live.model || cat?.model || 'Camera',
    facts,
    unit: [['Model', live.model], ['Sensor', live.sensor], ['Resolution', size], ['Pixel size', has(live.pixel_um) ? `${live.pixel_um} µm` : undefined], ['Bit depth', live.bits], ['Colour', live.color === undefined ? undefined : live.color ? 'colour' : 'mono'], ['Cooled', yn(live.cooled)], ['Serial', live.serial]],
    spec: cat ? [['Maker', cat.vendor], ['Model', cat.model], ['Sensor', cat.sensor], ['Sensor size', cat.sensor_size_mm], ['Resolution', cat.resolution], ['Pixel size', has(cat.pixel_um) ? `${cat.pixel_um} µm` : undefined], ['ADC', has(cat.adc_bits) ? `${cat.adc_bits}-bit` : undefined], ['Max fps, full frame', cat.max_fps_full_res], ['Full well', has(cat.full_well_e) ? `${cat.full_well_e} e⁻` : undefined], ['Read noise', has(cat.read_noise_e) ? `${cat.read_noise_e} e⁻` : undefined], ['High conversion gain from', cat.hcg_gain ?? undefined], ['Unity gain', cat.unity_gain], ['Peak QE', has(cat.qe_peak_pct) ? `${cat.qe_peak_pct} %` : undefined]] : [],
  }
}

function mountRows(live: MountLive = {}, cat: MountCatalog | null | undefined): { title: string; facts: string; unit: Row[]; spec: Row[] } {
  const facts = [live.tracking === undefined ? undefined : live.tracking ? 'tracking on' : 'tracking off', live.port].filter(has).join(' · ')
  return {
    title: cat?.model ? `${cat.vendor ? cat.vendor + ' ' : ''}${cat.model}` : live.model || 'Mount',
    facts,
    unit: [['Model', live.model], ['Port', live.port], ['Sky tracking', live.tracking === undefined ? undefined : live.tracking ? 'on' : 'off']],
    spec: cat ? [['Maker', cat.vendor], ['Model', cat.model], ['Payload', has(cat.payload_kg) ? `${cat.payload_kg} kg` : undefined], ['Weight', has(cat.weight_kg) ? `${cat.weight_kg} kg` : undefined], ['Modes', list(cat.modes)], ['Guide rates', list(cat.guide_rates)], ['Tracking rates', list(cat.tracking_rates)], ['Power', cat.power], ['Ports', list(cat.ports)]] : [],
  }
}

function Spec({ title, rows }: { title: string; rows: Row[] }) {
  const shown = rows.filter(([, v]) => has(v))
  if (!shown.length) return null
  return (
    <>
      <h3>{title}</h3>
      <dl className="specs">
        {shown.map(([k, v]) => (<div key={k}><dt>{k}</dt><dd>{String(v)}</dd></div>))}
      </dl>
    </>
  )
}

/**
 * Compact, collapsed-by-default card for the connected camera or mount: line art plus a few facts,
 * expanding to a datasheet. Shows only the fields it was given, so it works with any subset (or none).
 */
export function DeviceCard<K extends DeviceKind>({ kind, data }: { kind: K; data: Datasheets[K] }) {
  const cat = data.catalog
  const r = kind === 'camera' ? cameraRows(data.live as CameraLive, cat as CameraCatalog | null | undefined) : mountRows(data.live as MountLive, cat as MountCatalog | null | undefined)
  const notes = cat?.notes
  const hcgNote = kind === 'camera' ? (cat as CameraCatalog | null | undefined)?.hcg_note : undefined
  const sources = cat?.sources
  return (
    <details className="devcard">
      <summary>
        <img className="dev-img" src={deviceImage(kind, cat?.image)} alt="" width="72" height="45" />
        <span className="dev-main"><b>{r.title}</b>{r.facts && <span className="dev-facts">{r.facts}</span>}</span>
        <svg className="chev" viewBox="0 0 12 12" width="14" height="14" aria-hidden="true"><path d="M2 4l4 4 4-4" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" /></svg>
        <span className="sr-only">Datasheet</span>
      </summary>
      <div className="devcard-body">
        <Spec title="This unit" rows={r.unit} />
        <Spec title="Datasheet" rows={r.spec} />
        {!cat && <p className="hint-text">No datasheet is available for this device yet.</p>}
        {has(hcgNote) && <p className="hint-text">{hcgNote}</p>}
        {has(notes) && <p className="hint-text">{notes}</p>}
        {has(sources) && <p className="hint-text">Sources: {sources!.join('; ')}</p>}
      </div>
    </details>
  )
}
