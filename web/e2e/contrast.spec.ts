import { test, expect } from '@playwright/test'
import { readFileSync } from 'node:fs'

/**
 * Contrast of the colour tokens in styles.css, computed from the CSS custom properties of both themes (no browser, no server).
 * The floor: text 4.5:1, borders of controls, slider tracks, greyed text and chart strokes 3:1.
 */
const css = readFileSync(new URL('../src/styles.css', import.meta.url), 'utf8')

function tokens(selector: RegExp): Record<string, string> {
  const out: Record<string, string> = {}
  for (const m of css.matchAll(selector)) {
    for (const d of m[1].matchAll(/(--[a-z0-9-]+)\s*:\s*([^;]+);/g)) out[d[1]] = d[2].trim()
  }
  return out
}
const night = tokens(/(?:^|\n):root\s*\{([^}]*)\}/g)
const day = { ...night, ...tokens(/:root\[data-theme='day'\]\s*\{([^}]*)\}/g) }

const lin = (c: number) => { const s = c / 255; return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4 }
function lum(hex: string): number {
  const h = hex.replace('#', '')
  const f = h.length === 3 ? h.split('').map((x) => x + x).join('') : h
  const [r, g, b] = [0, 2, 4].map((i) => parseInt(f.slice(i, i + 2), 16))
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}
function ratio(a: string, b: string): number {
  const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p)
  return (x + 0.05) / (y + 0.05)
}

/** [foreground token, background token or hex, minimum ratio, what it is] */
const PAIRS: [string, string, number, string][] = [
  ['--fg', '--bg', 7, 'body text'],
  ['--muted', '--bg', 4.5, 'secondary text'],
  ['--muted', '--surface', 4.5, 'secondary text on a surface'],
  ['--faint', '--bg', 4.5, 'faint text'],
  ['--link', '--bg', 4.5, 'links'],
  ['--control-line', '--bg', 3, 'control border on the page'],
  ['--control-line', '--raised', 3, 'control border on a button'],
  ['--track', '--bg', 3, 'slider track'],
  ['--disabled-fg', '--raised', 3, 'greyed text on a button'],
  ['--disabled-fg', '--surface', 3, 'greyed text on a surface'],
  ['--disabled-fg', '--bg', 3, 'greyed text on the page'],
  ['--accent', '--bg', 3, 'accent on the page'],
  ['--ok', '--bg', 4.5, 'ok text'],
  ['--bad', '--bg', 4.5, 'bad text'],
  ['--warn', '--bg', 4.5, 'warn text'],
  ['--info', '--bg', 4.5, 'info text'],
  ['--accent-ink', '--accent', 4.5, 'text on the accent'],
  ['--chip-bad-ink', '--bad', 4.5, 'text on a bad chip'],
  ['--hist-r', '--plot-bg', 3, 'red histogram curve'],
  ['--hist-g', '--plot-bg', 3, 'green histogram curve'],
  ['--hist-b', '--plot-bg', 3, 'blue histogram curve'],
  ['--hist-m', '--plot-bg', 3, 'mono histogram bars'],
]

for (const [theme, t] of [['night', night], ['day', day]] as const) {
  test.describe(`contrast tokens, ${theme} theme`, () => {
    for (const [fg, bg, min, what] of PAIRS) {
      test(`${what}: ${fg} on ${bg} is at least ${min}:1`, () => {
        const f = t[fg], b = t[bg]
        expect(f, `${fg} is defined`).toMatch(/^#/)
        expect(b, `${bg} is defined`).toMatch(/^#/)
        const r = ratio(f, b)
        expect(r, `${f} on ${b} is ${r.toFixed(2)}:1`).toBeGreaterThanOrEqual(min)
      })
    }
  })
}
