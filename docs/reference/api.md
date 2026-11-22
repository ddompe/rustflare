# HTTP API

JSON over HTTP; errors are `400 {"error": "…"}`. The web UI uses only this API, so you can script a session with `curl` (see `scripts/smoke.sh`).

## State

| | |
|---|---|
| `GET /api/status` | full `Status` snapshot |
| `GET /api/ws` | WebSocket; pushes `Status` JSON on change (≤ ~7/s) |
| `GET /stream.mjpg[?fps=N]` | live preview (multipart MJPEG), optionally capped to N fps per viewer |
| `GET /snapshot.jpg` | latest preview frame |
| `GET /api/files`, `GET /files/<path>` | list / download captures (a PNG burst is listed as a folder with its frame count) |
| `GET /api/storage` | current storage location (free space, filesystem, availability) and candidate drives |
| `PUT /api/storage` | `{"path"}`: choose a location (a folder on a drive); refused while capturing |
| `PUT /api/optics` | `{"focal_length_mm"?,"pixel_size_um"?,"binning"}` |
| `GET/PUT /api/preview` | `{"stretch"}`: auto-stretch of the live view |

`Status` has `optics`, `sun` (located Sun: pos, radius, partial), `camera` (name, controls, fps, mean, saturation, histogram, size), `mount`, `tracking` (mode, x, y, radius, lost), `guiding` (state, calibration, target, error_px), `capture` (state, phase, done, total, file, last_file), `error`.

## Camera

| | body |
|---|---|
| `GET /api/cameras` | — |
| `POST /api/camera/open` | `{"driver","id"}` |
| `POST /api/camera/close` | |
| `PUT /api/camera/control` | `{"id","value"}` (ids come from `status.camera.controls`) |

## Mount

| | body |
|---|---|
| `GET /api/mount/drivers` | |
| `POST /api/mount/connect` | `{"driver":"zwo","device":"/dev/ttyACM0"}` or `{"driver":"simulator"}` |
| `POST /api/mount/disconnect` | |
| `POST /api/mount/tracking` | `{"enabled":true}` (selects the solar rate when enabling) |
| `POST /api/mount/slew` | `{"axis":"primary"\|"secondary","speed":8}` — signed multiples of sidereal; `0` stops |
| `POST /api/mount/stop` | stops all motion (does not change sky tracking) |

## Tracking & guiding

| | body |
|---|---|
| `POST /api/tracking/start` | `{"mode":"limb"}` (optional `"radius"` px; default from Optics), `{"mode":"sun_disc"}`, `{"mode":"anchor","x":640,"y":480}` |
| `POST /api/tracking/stop` | |
| `POST /api/guiding/calibrate` | needs tracking + mount sky tracking |
| `POST /api/guiding/start` | `{"center":true}` moves the target to the frame centre, otherwise holds the current position; needs calibration |
| `POST /api/guiding/stop` | |

## Capture

| | body |
|---|---|
| `POST /api/capture/single` | `{"name"?}` |
| `POST /api/capture/sequence` | `{"name"?,"frames"?,"seconds"?,"format"?}` (`format`: `ser` or `png`; at least one limit) |
| `POST /api/capture/bursts` | `{"name"?,"frames"?,"seconds"?,"interval_s","bursts"?,"format"?}`: a burst every `interval_s` (start to start); `bursts` omitted = until stopped |
| `PUT /api/capture/buffer` | `{"mb"}`: RAM buffer size (64–8192) |
| `POST /api/capture/flats` | `{"name"?,"frames","level"?}` (`level` 0.1–0.9, default 0.5) |
| `POST /api/capture/stop` | also forgets a paused recording |
| `POST /api/capture/pause` | pauses a running sequence or lane recording, keeping what is left |
| `POST /api/capture/resume` | starts the paused recording again (files named `…-resumed`) |
| `PUT /api/flats/applied` | `{"enabled"}`: apply the master flat to the live view (needs a master flat) |

## Mount: Go to Sun, site, home

| | body |
|---|---|
| `PUT /api/site` | `{"latitude","longitude"}` degrees, east-positive longitude; saved in the settings file |
| `POST /api/mount/goto-sun` | `{"confirm":true,"allow_unsynced_clock"?}`: sends site and time to the mount, slews to the Sun, then switches solar tracking on; refused without a site, below the horizon, with a capture running, or (unless overridden) when the Pi's clock is not NTP-synchronised. `status.goto` shows progress |
| `POST /api/mount/home` | `{"confirm":true}` |
| `POST /api/mount/stop` | also cancels a GoTo |
| `POST /api/mount/flip` | meridian flip assist: `{"record_only":true}` saves the pointing and the Sun's place; `{"confirm":true}` flips and re-centres; `{"confirm":true,"restore":true}` re-centres after a manual flip. Needs an idle camera |
| `PUT /api/mount/autoflip` | `{"enabled","record_min"?,"flip_min"?}`: unattended flip (minutes before the meridian to record, minutes after to flip). `status.auto_flip` carries `stage`, `message` and `resume_pending` |

`status` also carries `site`, `sky_sun` (altitude/azimuth/RA/Dec of the Sun now), `clock.ntp_synchronized` and `flats` (`available`, `applied`, `name`).

## Storage and captures

| | body |
|---|---|
| `GET /api/storage`, `PUT /api/storage` | candidates and the current location; `{"path"}` selects |
| `POST /api/storage/eject` | `{"mount_point"}`: flush, unmount and power off a removable drive; refused while capturing or writing |
| `GET /api/files` | the captures (date folder / name) |
| `GET /api/files/stats?path=` | SER header, measured fps and duration, sampled brightness, `mostly_dark`, `complete` |
| `POST /api/files/delete` | `{"path","confirm":true}`: deletes one capture and its notes file; refused while recording |
| `GET /api/datasheet/camera`, `/mount` | `{live, catalog}` for the device cards |

## Debugging

Read-only except the snapshot and the switch. See [Debugging and logs](debugging.md).

| | |
|---|---|
| `GET /api/debug/state` | engine internals: the Sun used for the mask, whether it is a prediction, learned drift, lane state, dead reckoning |
| `GET /api/debug/events?n=` | the last `n` events (tracking lost/found, guiding, exposure, lanes, captures, flips) |
| `GET /api/debug/mount?n=&skip_polls=1` | the last `n` commands sent to the mount and its replies |
| `GET /api/debug/logs` | every file in the logs folder, newest first; fetch them from `/files/<path>` |
| `POST /api/debug/snapshot` | saves the latest full frame and a note; returns their URLs |
| `PUT /api/debug/lost-frames` | `{"enabled"}`: keep the frame when tracking is lost (on by default) |

