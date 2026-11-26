# 20. iOptron mount driver

Date: 2026-11-26. Status: accepted (not yet verified on hardware).

## Context

Users own iOptron mounts (CEM70, and the HAE69C, which AsiAir drives with the CEM70 protocol). iOptron publishes its RS-232 command language (version 3.10 covers the CEM120, CEM70, GEM45, CEM40, GEM28 and CEM26 series) and USB mounts appear as a serial port.

## Decision

A `rustflare-mount` driver `ioptron`, generic over the serial I/O like the ZWO driver so the protocol is unit-tested against a mock. It uses the document's commands only: `:MountInfo#` on connect, `:GLS#` for state, `:GEP#` for pointing, `:SRA`/`:Sd`/`:MS1#` (then `:MS2#`) for Go to, `:ST0/1#`, `:RT0/1/2#`, `:Z?nnnnn#` guide pulses, `:SRn#` with `:m?#`/`:q?#` for slews, `:SG`, `:SUT`, `:SLO`, `:SLA`, `:SHE` for site and time. It is selected in the Aim tab; nothing else in the application knows which mount it is.

## Consequences

- Equatorial German-style mounts work like the ZWO driver for tracking, Go to and guiding, with no Bluetooth.
- The HAE69 is assumed to answer the same language; the model code is only used for the name, so an unknown four-digit code is accepted.
- It has been run against a mock only. The first real connection may show wrong signs (the document's text for the pad directions is garbled in places) or timing; the debug endpoints log every command.
- Sky-Watcher (SynScan / EQMOD, the motor controller protocol) is a separate driver, researched in `docs/project/research-skywatcher.md`.
