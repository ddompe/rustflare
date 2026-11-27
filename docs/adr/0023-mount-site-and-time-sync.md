# 23. The mount's own site and time: read back, compare, send

Date: 2026-11-27. Status: accepted (read and write used on a real iOptron HAE69C; the ZWO side reads the clock only).

## Context

Go to is computed by the mount from the site and the time it holds. A ZWO mount is sent them with every Go to Sun. The iOptron HAE69C has **no clock battery** and restarts from 2017-01-01 14:00 UTC every time it is switched on (seen on a real mount), so a Go to computed by the mount itself, or any later use of its own clock, is wrong until someone sets it. The owner could not see what the mount held.

## Decision

1. The driver can **read back** what the mount holds (`Mount::read_site_time`): the iOptron answers site and clock, a ZWO mount its clock only (`:GAST#`, in its local time). A driver that cannot says so and the UI greys the block.
2. The engine reads it on connecting and every 30 s (an iOptron command can take 300 to 600 ms, so not with the 2 s poll) and publishes `status.mount.site_time` with the Pi's own values next to it and a verdict: the clock may differ by 5 s and the place by 0.05 degrees; a place the mount does not report is never called a match.
3. A difference is a **note**, not a warning (Go to sends the Pi's site and time itself first). The notes chip carries *Send now* and *Show on Aim*; the Aim tab has *Mount time and place* with *Send site and time to the mount*. Sending is `POST /api/mount/sync-site-time`: set commands, nothing moves, refused without a saved site, while parked or moving.
4. **Once after connecting** the Pi sends its site and time by itself when they differ, if the setting is on (`PUT /api/mount/auto-site-time`, on by default), the Pi's own clock is synchronised to a time server (otherwise it could be the wrong clock), a site is saved and the mount is not moving. After that it is only done by hand. It is never done while a Go to or a flip runs.
5. The Pi's own clock is checked first: the block says when it is not synchronised, and nothing is sent automatically then.

## Alternatives considered

- Always send at connect: would overwrite a correct mount clock with a wrong Pi clock.
- Trust the mount's clock: not on a mount without a battery.
- Leave it to the hand controller: needs a visit to the mount every session.

## Consequences

- The first Go to of a session on a battery-less mount is right without a visit to the hand controller.
- A ZWO mount's site cannot be compared (its read-back is not documented here), so only its clock is checked.
- Whether the HAE69C's clock keeps ticking after a write is not yet known (the checklist asks for a second look after a minute).
