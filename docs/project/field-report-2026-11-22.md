# Field report: 2026-11-22

A day on the real Sun with a Lunt LS50 (H-alpha), a ZWO AM3 (Bluetooth), a Player One Apollo-M Mini and a Raspberry Pi 5, from about 15:00 to 20:10 UTC. The numbers come from `scripts/analysis/field_report.py` run on the logs copied off the Pi (events, mount commands, session logs); rerun it after any session. The software was updated about fifteen times during the day, so the figures are a record of the fixes as much as of the equipment.

## What the data says

- **Guiding got steadily better as the fixes landed.** Median guide error fell from 23 px in the first hour to 5.3 px in the last (p90 224 px to 14 px, worst case 1473 px to 18 px). The big early errors were outliers let into the guider, a calibration that under-read the guide rate by a quarter (the AM3 ends a guide command after 3 s), and no guiding at all during lane recordings. The last hour was also the cloudiest.
- **The Sun model stopped jumping.** Model steps of more than 100 px between consecutive seconds, per hour: 341, 32, 32, 4, then none. The remaining ones belong to the manual centring and the slides below.
- **Brief losses are normal and were hidden.** The tracker was lost 187 times, but the median loss lasted 0.9 s. With the 2 s debounce most never reached the screen. The 90th percentile is 15.6 s (cloud and leaves), the longest 3,588 s (the Sun behind the trees at the end of the day). Dead reckoning carried guiding through three cloud episodes (18:41 to 18:44) on a learned drift of 0.16 to 0.20 px/s.
- **Light varies a lot.** 28 % of the seconds were under 0.30 of full brightness (median 0.44; down to 0.003 behind trees). Detection was brightness-independent from the middle of the day.
- **The mount talks fine.** Over Bluetooth a command takes a median of 73 ms (p95 104 ms, worst 161 ms), with no errors in 12,138 commands. Guide commands average about 300 an hour (about five a minute, counting the 2.5 s chained pieces). The mount stopped tracking by itself at 17:41:10 UTC, 19 minutes past the meridian, as expected with the flip off.
- **Calibration is noisy on the secondary axis.** After the 3 s fix the primary rate is steady (14.8 to 18.8 px/s per 1x) but the secondary varies from 7.9 to 17.7 across six calibrations. A 4 s move at 0.5x gives about 20 to 30 px, and the limb-fit noise in shadow is a large part of that. The closed loop tolerates it, but a longer move or a repeat and median would be better (planned, see below).
- **The pier flip is visible in the calibration.** The primary direction turned from about 52° to about 247° (about 195°), while the secondary stayed near 160°: the mount reverses the sense of Dec commands with the pier side, and the image turned over, so the two cancel on one axis. Recalibrate after a flip, which the automatic flip already does.

## Still open

- **The slides** (#82): three about 330 px slides along one image direction before the polar alignment, and a fourth after it with no mount command in the log. Mechanical or optical; unresolved.
- **Capture stalls** (#86): 1 to 2.4 s gaps in a full-rate recording that the dropped counter does not show.
- **The Sun model in near-total darkness** (#79, #83): it wandered over 1,200 px in 20 minutes of cloud; fixed after the fact (nothing is measured below 3 % of the good-light brightness) and not yet seen on the Sun.

## The numbers


## Events

2342 events from 15:07:50 to 19:52:43 UTC.

| event | count |
|---|---|
| sun_held | 472 |
| sun_back | 471 |
| exposure | 395 |
| lane | 389 |
| tracking_lost | 190 |
| tracking_found | 187 |
| sun_predicted | 111 |
| sun_measured_again | 27 |
| guiding_stop | 23 |
| calibration_done | 15 |
| guiding_start | 15 |
| guiding_resumed | 14 |
| capture_start | 11 |
| capture_end | 11 |
| capture_abort | 5 |
| dead_reckoning_start | 3 |
| dead_reckoning_end | 3 |

Tracking lost 187 times; duration median 0.9 s, p90 15.6 s, longest 3588 s.

**Calibrations**

| time (UTC) | rate primary / secondary (px/s at 1x) | primary direction | secondary direction |
|---|---|---|---|
| 15:28:31 | 12.0 / 17.5 | 61° | 163° |
| 15:32:29 | 14.6 / 8.2 | 75° | 152° |
| 15:37:59 | 13.7 / 7.7 | 47° | 155° |
| 16:02:04 | 7.9 / 7.5 | 303° | 224° |
| 16:11:02 | 14.0 / 9.9 | 52° | 156° |
| 16:15:39 | 13.0 / 9.8 | 47° | 159° |
| 16:40:36 | 12.9 / 8.9 | 49° | 159° |
| 16:41:43 | 13.5 / 10.5 | 56° | 160° |
| 17:04:43 | 18.8 / 13.6 | 57° | 160° |
| 17:06:08 | 18.1 / 9.8 | 52° | 158° |
| 17:34:12 | 14.8 / 14.8 | 56° | 170° |
| 17:47:52 | 15.5 / 16.3 | 244° | 160° |
| 17:50:58 | 15.2 / 7.9 | 266° | 148° |
| 19:15:42 | 15.5 / 17.7 | 248° | 160° |
| 19:24:56 | 16.4 / 13.2 | 250° | 160° |

`dead_reckoning_start`: 18:41:49 {"drift_px_s": [0.16254200896078375, 0.07235495031374616]}; 18:42:32 {"drift_px_s": [0.18288447127405488, 0.06386034562373666]}; 18:43:39 {"drift_px_s": [0.19571326566219058, 0.06676857526090915]}

`capture_abort`: 16:20:15 {"job": "capture", "why": "stopped by user"}; 16:56:10 {"job": "capture", "why": "stopped by user"}; 17:21:14 {"job": "capture", "why": "stopped by user"}; 17:33:15 {"job": "capture", "why": "stopped by user"}; 17:41:41 {"job": "capture", "why": "stopped by user"}

## Mount command log

12138 commands from 17:33:35 to 20:09:45 UTC (unique by time and text). Round trip: median 73 ms, p95 104 ms, max 161 ms. Errors: 0.

| command | count |
|---|---|
| `:GU#` | 3719 |
| `:Gps#` | 3713 |
| `:Mge0000#` | 800 |
| `:Mgw0000#` | 800 |
| `:Mgn0000#` | 703 |
| `:Mgs0000#` | 703 |
| `:Rg0.2#` | 687 |
| `:Mgn3000#` | 389 |
| `:Mge3000#` | 302 |
| `:Rg0.8#` | 92 |
| `:Mgs3000#` | 58 |
| `:Mgw3000#` | 48 |

Guide commands (3 s timed pulses, chained for long corrections): 797 in 2.6 h, 306 per hour.

Mount tracking flag changes (from `:GU#`): 17:41:10 OFF, 17:42:53 on, 17:46:38 OFF, 17:46:40 on, 17:46:43 OFF, 17:46:51 on, 17:47:13 OFF, 17:47:15 on, 17:47:19 OFF, 17:47:21 on

## Session logs (tracking and guiding, once a second)

12525 samples from 15:07:56 to 19:42:57 UTC.

Guiding error while guiding (8390 samples): median 13.8 px, p90 127.8, p99 546.8, max 1473.

| hour (UTC) | samples | median error | p90 | max |
|---|---|---|---|---|
| 15:00 | 1359 | 23.4 | 223.9 | 1473 |
| 16:00 | 1304 | 18.7 | 381.7 | 555 |
| 17:00 | 2425 | 17.0 | 85.4 | 353 |
| 18:00 | 2300 | 11.4 | 54.1 | 429 |
| 19:00 | 1002 | 5.3 | 14.4 | 18 |

Light level (mean frame brightness 0..1): median 0.44, p5 0.008, min 0.003; 3461 samples under 0.30 (cloud or shadow).

Guide pulses logged: 2001.

Sun-model steps over 100 px between consecutive samples: 409 (per hour UTC: 15:00 341, 16:00 32, 17:00 32, 18:00 4).
