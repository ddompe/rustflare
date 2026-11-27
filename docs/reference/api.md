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
| `PUT /api/storage/reserve` | `{"gb": 2 to 100}`: how much of the internal disk a recording must leave free (default 8). `GET /api/storage` marks the internal disk with `internal: true` and its `reserve_bytes`, and lists it among the candidates |
| `PUT /api/storage` | `{"path"}`: choose a location (a folder on a drive); refused while capturing |
| `GET /api/mount/detect` | the serial ports, each with `driver` and `model` when a mount we drive answered a read-only question there (ZWO `:GVP#` at 9600 baud, iOptron `:MountInfo#` at 115200); a few hundred ms a port; no answers while a mount is connected |
| `PUT /api/profile` | `{id}`: what is imaged, one of `sun_white`, `sun_ha` (default), `moon`, `planet`, `other`. Saved in the settings file, reported as `status.profile`, used for the FITS `OBJECT` and by the UI for its words and its greyed controls; the API refuses nothing because of it |
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
| `PUT /api/camera/cooler` | any of `{"on":bool,"target_c":number,"fan":bool,"heater":bool,"immediate":bool}`; 400 when nothing is given, the camera has no cooler, or the control is missing. Switching **off** a cooler while the sensor is below 15 degrees does not switch it off at once: the target is raised 1 degree every 15 s first and the answer is `{"warming_up":true}`; `"immediate":true` skips that. A new `on` or `target_c` cancels a warm-up. |

`status.camera.cooler` is `{available, on, target_c, target_min_c, target_max_c, power_pct, temp_c, fan, heater, warming_up}`; `available` is false (and the rest empty) for an uncooled camera, and `fan` and `heater` are null where the camera has no such switch. The target is in whole degrees Celsius.

## Mount

| | body |
|---|---|
| `GET /api/mount/drivers` | |
| `POST /api/mount/connect` | `{"driver":"zwo","device":"/dev/ttyACM0"}` or `{"driver":"simulator"}` |
| `POST /api/mount/disconnect` | |
| `POST /api/mount/tracking` | `{"enabled":true,"rate"?:"solar"\|"lunar"\|"sidereal"}`: switches tracking on or off. Without `rate`, enabling keeps the rate the mount last had (a Go to Saturn leaves it sidereal); `status.mount.rate` says which |
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
| `PUT /api/capture/after-burst` | any of `{"mode":"video"\|"video_stack"\|"stack_only"\|"best_frames","stack_keep_percent":1-100,"raw_keep_percent":1-100,"margin":1-5}`: what happens after each burst ([guide](../guide/recording-modes.md)). `stack_only` and `best_frames` are refused (400) while the stacker is not connected. Returns the settings; `status.after_burst` has them with `stacker_connected`, `stacking`, `stacked`, `last` and the measured `stack_s_per_frame` |
| `POST /api/notices/ack` | `{"id"?}`: dismiss one notice, or all. `status.notices` lists what the user must know about (`level` error or warning, `text`, `at`, `acked`): frames lost, a recording stopped early, a stack that fell back to video |
| `POST /api/capture/flats` | `{"name"?,"frames","level"?}` (`level` 0.1–0.9, default 0.5) |
| `POST /api/capture/stop` | also forgets a paused recording |
| `POST /api/capture/pause` | pauses a running sequence or lane recording, keeping what is left |
| `POST /api/capture/resume` | starts the paused recording again (files named `…-resumed`) |
| `PUT /api/flats/applied` | `{"enabled"}`: apply the master flat to the live view (needs a master flat) |

## Mount: Go to Sun, site, home

| | body |
|---|---|
| `PUT /api/site` | `{"latitude","longitude"}` degrees, east-positive longitude; saved in the settings file |
| `POST /api/mount/goto` | `{"target":"sun"|"moon"|"mars"|"jupiter"|"saturn","confirm":true,"allow_unsynced_clock"?,"allow_below_horizon"?}`: like goto-sun for any target; tracks at the solar, lunar or sidereal rate on arrival. Planets and the Moon below the horizon need `allow_below_horizon`; the Sun never goes. `status.sky_targets` lists each one's altitude, azimuth, RA and Dec |
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
| `GET /api/files/list?path=<folder>` | one folder of the drive (`""` is the top): `{path, entries:[{name, path, dir, type, size, files, capped, modified, stack?, from?}]}`. `type` is `day`, `stacks`, `logs`, `hdr`, `prominence`, `lane`, `folder`, `recording`, `stack`, `fits`, `image`, `notes` or `file`. For a folder `size` and `files` are totals below it (`capped` when counting stopped early). `stack` (on a recording) and `from` (on a stack) link the two from `stacks/manifest.json`. Notes files, `.ready` markers and the thumbnail cache are not listed |
| `GET /api/files/archive?path=<folder or file>[&names=<JSON array>]` | the folder as one streamed `.tar` (known `Content-Length`); with `names` (a JSON array of names inside `path`) only those items, as `selection.tar`. 409 while a capture is running, except under `stacks/`; 400 if a name does not fit in tar |
| `GET /api/files/stats?path=` | SER header, measured fps and duration, sampled brightness, `mostly_dark`, `complete` |
| `POST /api/files/delete` | `{"path","confirm":true}`: deletes one file or folder (below a day folder) and its notes file; refused while recording, and for `stacks/manifest.json` |
| `GET /api/datasheet/camera`, `/mount` | `{live, catalog}` for the device cards |

## Debugging

Read-only except the snapshot and the switch. See [Debugging and logs](debugging.md).

| | |
|---|---|
| `GET /api/debug/state` | engine internals: the Sun used for the mask, whether it is a prediction, learned drift, lane state, dead reckoning |
| `GET /api/debug/events?n=` | the last `n` events (tracking lost/found, guiding, exposure, lanes, captures, flips) |
| `GET /api/debug/mount?n=&skip_polls=1` | the last `n` commands sent to the mount and its replies |
| `GET /api/debug/logs` | every file in the logs folders, newest first; drive files from `/files/<path>`, internal ones (`internal/...`) from `/api/debug/internal/<day>/<file>` |
| `POST /api/debug/snapshot` | saves the latest full frame and a note; returns their URLs |
| `PUT /api/debug/lost-frames` | `{"enabled"}`: keep the frame when tracking is lost (on by default) |


`home_when_done` (boolean, default false) can be added to `/api/capture/sequence`, `/bursts` and `/lanes`: when the recording finishes by itself the mount is sent home (guiding stops first). `status.capture.eta_s` is the estimated seconds until the running recording ends (null when it runs until stopped).

## Stacking

The stacker (`rustflare-stack`, see [Stacking](../guide/stacking.md)) is a separate program; these endpoints read its files and edit its settings.

| Endpoint | |
|---|---|
| `GET /api/stack` | `{service, restart, status, config, stacks, total, calibration}`: whether the `rustflare-stack` service is active (`restart` is `{attempts, error}`: how many times the server started it again since it last saw it running, and why the last try failed; the server does this itself every 20 s or longer, only where the unit exists and the stacker is switched on), its `stack-status.json`, its settings, the newest 48 stacks (source, output, frames kept, thumbnail name, `path` to download under `/files/`) and the raw dark and flat recordings on the drive |
| `PUT /api/stack/config` | any of `enabled`, `policy` (`when_idle`, `between_bursts`, `always`), `min_gap_s`, `keep_percent` (1 to 100), `multipoint`, `threads` (1 to 4), `float`, `flat`, `darks` (paths relative to the storage folder), `include_unmarked`, `disc_mask` (black disc on prominence stacks), `disc_scale` (0.8 to 1.2); returns the settings now in force |
| `POST /api/stack/restack` | `{"path":"stacks/...","confirm":true}`: deletes the stack (or every stack in the folder) and removes it from the manifest so the stacker makes it again from its recording with the current settings. 409 while capturing or while the stacker is working; 400 if a recording is gone. Returns `{queued, deleted}` |
| `GET /api/stack/thumb/<path>` | a PNG preview (at most 640 px wide, auto-stretched, display only) of a stack under `stacks/`, made on first request |
