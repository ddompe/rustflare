# Research: Sky-Watcher mounts (Wave 100i / 150i, SynScan, EQMOD)

Question: how hard is it to drive a Sky-Watcher mount (the Wave strain-wave mounts first) from RustFlare over USB, using what the SynScan and EQMOD drivers already know? Written 2026-11-26 from public documents; **nothing here has been tried on a mount**. What is read from a document is marked *(doc)*, what comes from forum posts or a vendor page *(report)*, what is a guess *(guess)*.

## Two protocols, two drivers

| | Motor controller protocol | SynScan hand controller protocol |
|---|---|---|
| Spoken to | the motor boards directly (what EQMOD and INDI's *EQMod* driver speak, and what the SynScan app speaks over Wi-Fi) | the SynScan hand controller (INDI's *SynScan* driver) |
| Document | *Sky-Watcher Motor Controller Command Set* (public PDF) *(doc)* | *SynScan Serial Communication Protocol v3.3* (public PDF) *(doc)* |
| Link | TTL serial at the mount, an EQDIR cable, a USB port on the mount (EQ6-R, AZ-EQ6, EQ8, CQ350 have one; the Wave has a USB-B socket *(doc: Sky-Watcher spec page)*), or Wi-Fi as UDP port 11880 *(report)* | RS-232 on the hand controller, 9600 baud, 8N1 *(doc)*, or the SynScan Wi-Fi dongle |
| What the master computes | everything: the motor only counts steps | the hand controller does alignment and GoTo; the master sends coordinates |

The Wave has no hand controller in the box (the SynScan handset is an accessory; the mount takes it on a socket *(doc: spec page: "SynScan Handset" socket)*). A USB cable straight to the mount, as AsiAir uses *(report: "Works seamlessly with ASI Air (USB-B cable included)")*, can only be the **motor controller protocol** unless the mount emulates a hand controller. *(guess, to be checked first, see the probe below.)*

Forum reports say the INDI *SynScan* and *EQMod* drivers and the ASCOM EQMOD driver work with the Wave over USB, and that EQMOD needed adjustment for the strain-wave gearing *(report: Cloudy Nights "Wave 150i and indi", Stargazers Lounge "SW 150i and EQMOD")*. That favours a direct motor-controller driver (the EQMod way).

## The motor controller protocol *(doc, read from the command set PDF)*

- **Framing.** Command `:` + command letter + channel (`1` = RA/azimuth, `2` = Dec/altitude, `3` = both for some) + 0 to 6 data characters + carriage return (0x0D). A second `:` before the CR restarts the command. Reply `=` + data + CR, or `!` + one error digit + CR (0 unknown command, 1 length error, 2 motor not stopped, 3 invalid character, 4 not initialised, 5 driver sleeping, 6 PEC training running, 7 no valid PEC data). All ASCII.
- **Numbers.** Hexadecimal, little-endian by byte: 0x123456 is sent as `563412`; 16 bits `3412`; 8 bits `12`. Positions are offset by 0x800000.
- **Two motion modes.** *Goto*: set the target increment (`:H`), start (`:J`); poll the status (`:f`) until it stops. *Speed (tracking)*: write the **T1 preset** (`:I`, the step period) and start (`:J`); the master changes the speed by writing a new preset. `:G` selects the mode and direction (bit 0 direction, bit 1 north/south, bit 2 fast/slow...). The motor must be fully stopped before the mode is changed, and goes back to speed mode after a stop.
- **Inquiries.** `:a` counts per revolution (CPR) of each axis, `:b` the timer interrupt frequency, `:g` the high-speed ratio, `:j` the position, `:f` the status (running/stopped, goto/tracking, direction, fast/slow, blocked, initialised), `:e` the board version (an `EQ...` or `AZ...` mount type and firmware), `:D` the sidereal period, `:s` the PEC period. `:F` finishes the initialisation (`:F1`, `:F2`) and `:E` sets the position.
- **The arithmetic is the master's.** Steps per degree = CPR / 360. For a speed v in deg/s the T1 preset is `timer_freq * 360 / (v * CPR)`; for speeds above about 128x sidereal the controller moves `b` micro-steps per interrupt (`b` = the high-speed ratio) and the preset is multiplied by `b`.
- **Not in the document:** which firmware the Wave runs, how the Wave's USB-B port presents itself (CDC serial? at what baud?), and anything about the strain-wave specifics.

## What a RustFlare driver needs (and how it maps)

| `Mount` trait | Motor controller implementation |
|---|---|
| `connect` | open the port, `:e1`/`:e2` (model, firmware), `:a1`/`:a2` (CPR), `:b1`, `:g1`/`:g2`, `:F1`, `:F2` |
| `state`, `equatorial` | `:j1`, `:j2` and `:f1`, `:f2`; counts to angles with the CPR; the pointing needs a model of where the axes are (below) |
| `set_tracking`, `set_tracking_rate` | speed mode on RA with a T1 preset for sidereal (15.041 arcsec/s), solar (about 15.0) or lunar rate; stop = `:K1` |
| `slew` (pad) | speed mode with a preset for the chosen multiple of sidereal; up to the mount's top speed (the Wave: 2400x, 10 deg/s *(doc: spec page)*) |
| `goto_equatorial` | convert RA/Dec to axis counts, `:H` and `:J` in goto mode, poll `:f`, then resume tracking |
| `guide` | change the T1 preset by a fraction for a time (the guide rate), or `:P` for the 0.125x to 1x auto-guide rate register |
| `go_home`, flip | goto to the home counts; a flip is a goto to the other side of the pier |

**The one hard part is the pointing model.** The motor board only knows counts. EQMOD keeps a sync/alignment model; the simplest honest version for RustFlare is: assume the mount starts at its home position (counterweight down, pointing at the pole, which is a defined count) or let the user sync once (point at the Sun, press Sync) and keep the offset. With a polar-aligned equatorial mount the transform is a rotation plus the hemisphere; the site's latitude and the sidereal time are already in RustFlare. An alt-az mode (the Wave supports it) needs a different transform and field rotation is the user's problem, as it already is for the AM3.

Effort *(guess)*: the protocol layer and unit tests against a mock, about a day, like the iOptron driver. The pointing model, solar tracking verification and guiding behaviour need a person at a mount, probably two sessions.

## What to do first: a 5 minute probe on the friend's Wave

`scripts/probe-skywatcher.py` (stdlib only, needs no installation) sends **read-only** inquiries and prints what answers, so we learn the protocol and the port before writing a line of the driver:

1. Connect the Wave's USB-B to a computer (a Pi, a Mac or a Linux laptop); note the device (`/dev/ttyACM0`, `/dev/ttyUSB0`, `/dev/cu.usbmodem...`) and `lsusb` / System Information output.
2. `python3 scripts/probe-skywatcher.py /dev/ttyACM0` tries the motor controller protocol at 115200 and 9600 baud (`:e1`, `:e2`, `:a1`, `:a2`, `:b1`, `:g1`, `:f1`, `:j1`) and the SynScan hand controller echo (`Kx`, `e`, `V`), and prints every reply. It never starts a motor, never writes a position and never sends a start/goto command.
3. Send the output, plus the mount's firmware version from the SynScan app (Settings), and we choose the driver.

## Other ways in

- Wi-Fi: the Wave and other Sky-Watcher mounts take the SynScan Wi-Fi protocol (the motor controller protocol as UDP on port 11880 *(report)*). A UDP transport under the same driver would also cover the AZ-GTi and the Wi-Fi mounts, the way Bluetooth sits under the ZWO driver (ADR 0015).
- INDI itself: running `indiserver` with `indi_eqmod_telescope` or `indi_synscan_telescope` next to RustFlare and talking INDI to it is possible but means a second program and a second mount model; the point of RustFlare's drivers is that one process owns the mount (see AGENTS.md).

## Sources

- Sky-Watcher, [Motor Controller Command Set](https://inter-static.skywatcher.com/downloads/skywatcher_motor_controller_command_set.pdf) (read)
- Sky-Watcher, [SynScan Serial Communication Protocol](https://www.teleskop-spezialisten.de/shop/media/Bedienungsanleitung_Handbuch_englisch/SkyWatcher%20instruction%20user%20Manual%20SynScan%20Serial%20Communication%20Protocol.pdf) (found, not yet read)
- Sky-Watcher, [Wave 150i](https://skywatcher.com/product/wave-150i/) and [Wave 100i](https://skywatcher.com/product/wave-100i/) specification pages (read)
- INDI, [Sky-Watcher telescopes](https://indilib.org/telescopes/skywatcher/) and the `SkywatcherAPIMount` class (named, not read)
- Cloudy Nights, [Wave 150i and indi](https://cloudynights.com/topic/939935-wave-150i-and-indi); Stargazers Lounge, [Skywatcher SW 150i and EQMOD](https://stargazerslounge.com/topic/430459-skywatcher-sw-150i-and-eqmod/)
