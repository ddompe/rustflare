/** Pixel-level histogram (sqrt scale so small populations stay visible) with a clipping warning. */
export function Histogram({ bins, saturated }: { bins: number[]; saturated: number }) {
  if (!bins.length) return null
  const max = Math.max(...bins.map(Math.sqrt), 1e-6)
  const w = 100 / bins.length
  const clipped = saturated > 0.001
  return (
    <div className="hist">
      <svg viewBox="0 0 100 30" preserveAspectRatio="none" role="img" aria-label="Histogram of pixel brightness">
        {bins.map((b, i) => {
          const h = (Math.sqrt(b) / max) * 30
          const last = i === bins.length - 1
          return <rect key={i} x={i * w} y={30 - h} width={w * 0.9} height={h} fill={last && clipped ? '#ff7466' : '#7dd3fc'} />
        })}
      </svg>
      <div className={`hist-msg ${clipped ? 'bad' : 'ok'}`} role={clipped ? 'alert' : undefined}>
        {clipped ? `Clipping: ${(saturated * 100).toFixed(1)}% of pixels at full scale. Reduce exposure or gain.` : 'No clipping'}
      </div>
    </div>
  )
}
