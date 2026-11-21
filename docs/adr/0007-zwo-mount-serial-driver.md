# 0007. ZWO mount: direct serial protocol, solar rate, guide-rate fix

- Status: Accepted — protocol logic unit-tested against a mock port; **not yet run against a real AM3/AM5 in this project** (upstream code was tested on AM3 and AM5)
- Date: 2026-11-21

## Context

Vidoxide includes a direct serial driver for ZWO AM3/AM5 mounts (ZWO serial protocol v1.8). Reviewing it for reuse found:

1. **Guide rate pinned to 0.9×.** `format!(":Rg{:.2}#", a1.max(a2.max(0.9)))` can never produce less than 0.9, so the requested guide speed is ignored. The mount accepts 0.1–0.9 with one decimal (INDIGO's driver clamps to that range and prints `%.1f`).
2. **No tracking-rate selection.** Sidereal rate is always used. INDIGO's ZWO driver confirms `:TQ#` (sidereal), `:TS#` (solar), `:TL#` (lunar).
3. `stop()` disabled sky tracking as a side effect, which would silently stop the mount when the server restarts or a user presses "stop".

## Decision

Port the driver with those three changes, generic over `Read + Write` so the protocol is unit-tested with a mock serial port (`guide_rate_is_clamped_not_pinned`, `solar_rate_and_slew`, …).
Enabling tracking from the UI first selects the **solar** rate. The serial path comes from configuration/UI (e.g. `/dev/ttyACM0`, or better a stable `/dev/serial/by-id/…`); the `serialport` crate is built without its `libudev` feature to keep cross-builds simple, so there is no automatic port discovery.

## Consequences

- The mount has one guide rate for both axes; the guider's per-axis proportions are therefore approximate, but guiding is closed-loop so this is acceptable.
- Disconnecting sends `:Q#` (stop motion) but leaves sky tracking as it was.
- On first use with real hardware, verify: model reply to `:GVP#`, the `:TS#` rate, and guide direction via the calibration routine.
