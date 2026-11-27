/** The three curves differ in line style as well as hue (solid, dashed, dotted), and each channel has its letter in the legend and on its clip marker.
 *  The colours are theme variables (--hist-r/g/b) so every stroke keeps at least 3:1 against the plot in the night and the day theme. */
const CH = [
  { k: 'R', name: 'red', fill: 'var(--hist-r)', dash: undefined },
  { k: 'G', name: 'green', fill: 'var(--hist-g)', dash: '7 4' },
  { k: 'B', name: 'blue', fill: 'var(--hist-b)', dash: '1.5 4' },
] as const

/** A step-area path over the 100 x 30 box for one channel (sqrt scale, so small populations stay visible). */
function area(bins: number[], max: number): string {
  const w = 100 / bins.length
  let d = 'M0 30'
  bins.forEach((b, i) => { const y = 30 - (Math.sqrt(b) / max) * 30; d += `L${(i * w).toFixed(2)} ${y.toFixed(2)}L${((i + 1) * w).toFixed(2)} ${y.toFixed(2)}` })
  return d + 'L100 30Z'
}

/**
 * The bars of a histogram: a single blue-grey series for a mono camera; for a colour camera three overlaid curves (red, green, blue)
 * with a stroke so they read in sunlight, and a marker at the right edge for every channel that clips.
 */
export function HistPlot({ bins, rgb, clipped, className, hidden }: { bins: number[]; rgb?: number[][] | null; saturatedRgb?: number[] | null; clipped: boolean; className?: string; hidden?: boolean }) {
  const a11y = hidden ? { 'aria-hidden': true as const } : { role: 'img' as const, 'aria-label': rgb ? 'Histogram of pixel brightness per colour channel' : 'Histogram of pixel brightness' }
  if (rgb && rgb.length === 3 && rgb[0].length) {
    const max = Math.max(...rgb.flat().map(Math.sqrt), 1e-6)
    return (
      <svg className={`${className ?? ''} hist-rgb`.trim()} viewBox="0 0 100 30" preserveAspectRatio="none" data-testid="hist-rgb" {...a11y}>
        {CH.map((c, i) => <path key={c.k} d={area(rgb[i], max)} fillOpacity={0.3} style={{ fill: c.fill, stroke: c.fill }} strokeWidth={2} strokeDasharray={c.dash} strokeLinecap="round" vectorEffect="non-scaling-stroke" data-channel={c.k} />)}
      </svg>
    )
  }
  const max = Math.max(...bins.map(Math.sqrt), 1e-6)
  const w = 100 / bins.length
  return (
    <svg className={className} viewBox="0 0 100 30" preserveAspectRatio="none" {...a11y}>
      {bins.map((b, i) => {
        const h = (Math.sqrt(b) / max) * 30
        const last = i === bins.length - 1
        return <rect key={i} x={i * w} y={30 - h} width={w * 0.9} height={h} style={{ fill: last && clipped ? 'var(--bad)' : 'var(--hist-m)' }} />
      })}
    </svg>
  )
}

/** "R 1.2%, B 0.4%" for the channels at full scale, or '' when none are. */
export function clipText(sat: number[] | null | undefined): string {
  if (!sat) return ''
  return CH.map((c, i) => (sat[i] > 0.001 ? `${c.k} ${(sat[i] * 100).toFixed(1)}%` : '')).filter(Boolean).join(', ')
}

/** Pixel-level histogram with a clipping warning. A colour camera gets one curve per channel, in its colour. */
export function Histogram({ bins, saturated, rgb, saturatedRgb, planetClip }: { bins: number[]; saturated: number; rgb?: number[][] | null; saturatedRgb?: number[] | null; planetClip?: { clipped: boolean; value: string } | null }) {
  if (!bins.length) return null
  // the planet profile judges clipping by the brightest part of the picture (a planet is a tenth of a percent of the frame)
  const clipped = planetClip ? planetClip.clipped : saturated > 0.001
  const which = clipText(saturatedRgb)
  return (
    <div className="hist">
      <HistPlot bins={bins} rgb={rgb} saturatedRgb={saturatedRgb} clipped={clipped} />
      {rgb && saturatedRgb && (
        <div className="hist-clip" aria-hidden="true">
          {CH.map((c, i) => (saturatedRgb[i] ?? 0) > 0.001 && <b key={c.k} data-clip={c.k} style={{ background: c.fill }}>{c.k}</b>)}
        </div>
      )}
      <div className="hist-foot">
        <div className={`hist-msg ${clipped ? 'bad' : 'ok'}`} role={clipped ? 'alert' : undefined}>
          {planetClip
            ? (clipped ? `Clipping: the brightest part of the picture (the planet) is at ${planetClip.value} of full scale. Reduce exposure or gain.` : 'No clipping')
            : clipped
            ? (which ? `Clipping: ${which} of the ${which.includes(',') ? 'channels' : 'channel'}'s pixels at full scale. Reduce exposure or gain.` : `Clipping: ${(saturated * 100).toFixed(1)}% of pixels at full scale. Reduce exposure or gain.`)
            : 'No clipping'}
        </div>
        {rgb && (
          <div className="hist-legend" aria-hidden="true">
            {CH.map((c) => (
              <span key={c.k}>
                <svg viewBox="0 0 26 10"><line x1="2" y1="5" x2="24" y2="5" style={{ stroke: c.fill }} strokeWidth="2.4" strokeDasharray={c.dash} strokeLinecap="round" /></svg>
                {c.k}
              </span>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
