# 20. iOptron mount driver

Date: 2026-11-26. Status: accepted. A real HAE69C has been connected (see Consequences); guiding and Go to Sun are not yet verified.

## Context

Users own iOptron mounts (CEM70, and the HAE69C, which AsiAir drives with the CEM70 protocol). iOptron publishes its RS-232 command language (version 3.10 covers the CEM120, CEM70, GEM45, CEM40, GEM28 and CEM26 series) and USB mounts appear as a serial port.

## Decision

A `rustflare-mount` driver `ioptron`, generic over the serial I/O like the ZWO driver so the protocol is unit-tested against a mock. It uses the document's commands only: `:MountInfo#` on connect, `:GLS#` for state, `:GEP#` for pointing, `:SRA`/`:Sd`/`:MS1#` (then `:MS2#`) for Go to, `:ST0/1#`, `:RT0/1/2#`, `:Z?nnnnn#` guide pulses, `:SRn#` with `:m?#`/`:q?#` for slews, `:SG`, `:SUT`, `:SLO`, `:SLA`, `:SHE` for site and time. It is selected in the Aim tab; nothing else in the application knows which mount it is.

## Consequences

- Equatorial German-style mounts work like the ZWO driver for tracking, Go to and guiding, with no Bluetooth.
- The HAE69 answers the same language: a real HAE69C answers `0066` to `:MountInfo#`. The model code is only used for the name and the top slew speed, so an unknown four-digit code is accepted (without a top speed).
- Timing was the surprise: the HAE69C takes 300 ms or more to answer `:ST1#` and `:qR#`, so the reply timeout is 2 s and stray input is dropped before each command. It has no clock battery, which led to [ADR 0023](0023-mount-site-and-time-sync.md). Slews, Send home, Search home, tracking, and the site and time have been used on it; the pad's RA and Dec signs, guide pulses and a meridian pass are still to check (the document's text for the pad directions is garbled in places; the debug endpoints log every command).
- The driver reports its own slew speeds and whether it can search for home, so the UI needs no iOptron knowledge.
- Sky-Watcher (SynScan / EQMOD, the motor controller protocol) is a separate driver, researched in `docs/project/research-skywatcher.md`.
