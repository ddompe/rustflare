# 22. A fixed view: the page never scrolls, messages sit on the picture

Date: 2026-11-27. Status: accepted (covered by the phone and desktop browser tests, which run on the simulators; not yet judged in sunlight on the phone).

## Context

Early versions let the whole page scroll. On a phone the live picture, the Start button and the warnings moved off the screen as soon as a panel grew, a warning appearing moved every control under it, and in sunlight the person at the telescope could not find Stop. Rounds of UI review in 0.7.0 and 0.8.1 to 0.8.2 turned the fixes into a rule.

## Decision

1. **The page does not scroll.** The header (Files, theme, Hide controls), the picture, the tab bar and one status line at the bottom have fixed places; only the controls of the selected tab scroll, inside their own panel.
2. **The picture's size does not depend on what is shown.** Its height is a share of the screen, set per tab, and does not change when a warning, a note or the Start strip comes or goes.
3. **Messages that come and go sit on the picture, taking no height:** warnings that matter (clipping, no target, mount parked, calibrating) are their own chips, everything else worth knowing (a target not found yet, a mount clock that differs, what the last recording saved, the *Set up N of 5* steps) is folded into one notes chip that leads with its key words and is as serious as its most serious note. The status bar at the bottom is one reserved line (tap to read all of it, *Dismiss* to remove); the software version sits at its right edge.
4. **Picture tools are named in words** in one *Tools* sheet on the picture (zoom, prominence view, stretch, overlays, widgets, display), and a tool that does not apply is greyed with its reason, as in [ADR 0021](0021-use-profiles.md).
5. **Start and Stop are always reachable:** the Start strip (Start, Snap, and one line saying what the recording keeps) is under the picture on every tab while the camera is open and idle; during a recording a Stop stays on screen even with the controls hidden (*Hide controls*, *Show controls*).

## Alternatives considered

- A scrolling page with sticky parts: in narrow landscape the sticky header covered the tab bar (fixed in 0.8.2), and the picture still changed size.

## Consequences

- Layout is a constraint on every new control: it must fit the panel, say why it is greyed, and put anything that comes and goes on the picture as a chip, not above or below it.
- The Playwright tests (`web/e2e/round2.spec.ts`, `round3.spec.ts`) check the sizes and the clipping at several phone and desktop viewports, with a wide font as well; changing the layout means changing them.
- The manual screenshots in `docs/img/manual/` predate this layout and were not regenerated.
