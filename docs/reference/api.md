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
| `PUT /api/profile` | `{id}`: what is imaged, one of `sun_white`, `sun_ha` (default), `moon`, `planet`, `other`. Saved in the settings file, reported as `status.profile`, used for the FITS `OBJECT`, the target of the meridian flip and its tracking rate (`flip_target`), the expected size, the default name of an unnamed recording (`Moon`, the planet's name), which measurer finds the target (disc and limb for the Sun and Moon, the brightest small disc for a planet, none for `other`), the clipping rule (the planet: `camera.peak`) and by the UI for its words and its greyed controls; the API refuses nothing because of it |
| `PUT /api/optics` | `{"focal_length_mm"?,"pixel_size_um"?,"binning"}` |
| `GET /api/optics/setups` | `{active, modified, setups: [...]}`: the named optical setups (trains) in the order saved, the one in use (`null` when none; nothing is created by itself) and whether the current optics or profile differ from it. A setup is `{name, profile, focal_length_mm, focal_multiplier, pixel_size_um, binning, aperture_mm, notes, flat, dark}`; `profile` is one of the profiles or `null` (applying leaves the profile alone), `pixel_size_um` is the override or `null`, `flat` and `dark` name the master files |
| `PUT /api/optics/setups/{name}` | saves (creates or replaces) a setup. Fields missing from the body come from the optics, profile, flat and dark in use now, so an empty body is "save as". Names are trimmed, 1 to 40 characters, no control characters and no `/`, unique; the ranges are those of `PUT /api/optics`; `aperture_mm` 5 to 5000; `notes` at most 500 characters; at most 50 setups; 400 on anything else. The saved setup becomes the active one when it equals what is in use now (or was already active); `"activate": false` in the body prevents that. `"rename_to": "New name"` renames the setup called `{name}` in place (same position in the list, still the active one if it was; 404 if `{name}` does not exist, 400 if the new name is taken or invalid) |
| `POST /api/optics/setups/{name}/apply` | makes it the current optics (the same validation and effect as `PUT /api/optics`: the expected radius and the tracker follow), switches to its profile when it has one (the same path as `PUT /api/profile`), re-selects its flat and dark when named and present, and makes it the active setup. Returns `{applied, before, after, not_restored}`: `before` and `after` are `{profile, focal_length_mm, focal_multiplier, effective_focal_mm, binning, scale_arcsec_per_px, expected_radius_px}`, `not_restored` lists `{what, name, reason}` for a flat or dark that could not be selected (never an error). Like `PUT /api/optics` it is not refused while recording. Moves no mount, starts nothing. 404 for an unknown name |
| `DELETE /api/optics/setups/{name}` | forgets a setup (the optics in use stay; if it was active nothing is active afterwards). 404 for an unknown name |
| `GET/PUT /api/preview` | `{"stretch"}`: auto-stretch of the live view |

`Status` has `optics`, `sun` (located target: pos, radius, partial; for the Moon profile the Moon, for the planet profile the planet, absent for Other), `camera` (name, controls, fps, mean, saturation, `peak`, histogram, size; for a colour camera also `histogram_rgb` and `saturated_rgb`), `mount`, `tracking` (mode, x, y, radius, lost), `guiding` (state, calibration, target, error_px), `capture` (state, phase, done, total, file, last_file), `error`.

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
| `POST /api/tracking/start` | `{"mode":"planet"}` (centroid of the brightest small disc: a planet), `{"mode":"limb"}` (optional `"radius"` px; default from Optics), `{"mode":"sun_disc"}`, `{"mode":"anchor","x":640,"y":480}` |
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
| `POST /api/mount/sync-site-time` | sends the Pi's saved site and clock to the mount (it moves nothing, so no `confirm`) and reads them back; answers `{"site_time": ...}`. Refused (400, with the reason) without a mount, without a saved site, while the mount is parked or slewing. Logs a `mount_site_time_sync` event |
| `POST /api/debug/sim-mount-clock` | test hook, simulated mount only: `{"time_offset_s"?, "latitude_deg"?, "longitude_east_deg"?}` sets what the simulated mount holds and re-reads it, to exercise a mismatch |
| `POST /api/mount/home` | `{"confirm":true}` |
| `POST /api/mount/stop` | also cancels a GoTo |
| `POST /api/mount/flip` | meridian flip assist: `{"record_only":true}` saves the pointing and the Sun's place; `{"confirm":true}` flips and re-centres; `{"confirm":true,"restore":true}` re-centres after a manual flip. Needs an idle camera |
| `PUT /api/mount/autoflip` | `{"enabled","record_min"?,"flip_min"?}`: unattended flip (minutes before the meridian to record, minutes after to flip). `status.auto_flip` carries `stage`, `message` and `resume_pending` |

`status.mount.site_time` is what the mount itself holds against the Pi's, read on connect and every 30 s: `{read_at_unix, mount:{latitude_deg, longitude_east_deg, unix_secs, utc_offset_minutes}, pi:{...}, time_diff_s (mount minus Pi), place_diff_deg, time_ok, place_ok, ok, why}`; fields the mount does not report are `null` (a ZWO mount reports its clock only), and the whole object is `null` when the driver cannot read them or nothing was read yet. `ok` is false when the clock differs by more than 5 s or the place by more than 0.05 degrees. The Pi's NTP state is `status.clock.ntp_synchronized`.

`status` also carries `site`, `sky_sun` (altitude/azimuth/RA/Dec of the Sun now), `target` (`{id, name}`: what the profile images; null for `other` and for `planet` until a planet was gone to), `flip_target` (the body the meridian flip follows: `{id, name, ra_hours, dec_deg, hour_angle_h, minutes_to_meridian, rate}`; for the Sun the same numbers as `sky_sun`, for the Moon its own hour-angle rate, `rate` is what is restored after a flip), `optics.target_radius_arcsec` (the Sun's, the Moon's or the planet's apparent radius), `optics.setup` (`{active, modified}`: the optical setup in use), `clock.ntp_synchronized` and `flats` (`available`, `applied`, `name`).

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
