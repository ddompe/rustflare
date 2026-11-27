# The screen

The **live view** is on top with its overlays. The five **tabs** are at the bottom and the controls of the selected tab sit in between. The tabs follow a session:

1. [Camera](camera.md): open the camera, set exposure and gain, flats and darks.
2. [Aim](aim.md): connect the mount, Go to Sun, meridian flip.
3. [Track](track.md): follow the Sun and correct drift.
4. [Record](record.md): single frames, sequences, bursts, files.
5. [More](more.md): display, site, storage, session log, updates.

A dot on a tab shows whether that step is ready (green), needs attention (yellow) or has not been started.

## Overlays

Overlays sit on the picture, are semi-transparent and can be dragged anywhere. Tap one to shrink it to one line. The grid button in the picture's tool column switches each one on or off and sets their opacity.

- **Status**: camera fps, screen fps (the preview rate your phone receives), disk MB/s while recording, exposure, gain, sensor temperature, mount state, and whether a flat or dark is applied.
- **Recording** and **RAM buffer**: appear while recording.
- **Guiding**: the average error and a one-minute graph while guiding.
- **Histogram**.
- **Mask size**: with the Prominence view on, a slider for the size of the black disc (90 to 120 % of the Sun's radius). It starts over the middle of the picture, which the mask blacks out anyway; drag it elsewhere if you like. The same size is used for a prominence lane that records with the coronagraph.
- **Focus score**: with Focus assist on, the score, the best so far and a short history. On a phone it starts as one line; tap it for the graph.
- **Meridian flip**: appears when the automatic flip is on and the Sun is within 15 minutes of the meridian, and while it flips. It says what is going on and whether a recording is paused.

The **eye** button in the picture's tool column hides every drawn overlay (crosshair, rings, Sun marks) so you see the bare image. In display settings the Sun edge circle and the centre dot with its arrow are separate switches.

The slider button opens display settings: auto-stretch, brightness, contrast, gamma, colour tint and bullseye. These change only what you see, never what is recorded.

## Prominence view

The round **Prominence view** button in the picture's tool column blacks out the Sun's disc (display only). Raise exposure or gain and watch the faint structure at the limb appear, so you can judge the exposure you want for prominences. It does not stretch the image, so what you see is the real brightness. Turn on **Auto-stretch** if you want it brightened: it has its own button in the picture's tool column (the curve icon) and a switch in More > Display, and both show the same setting for everyone watching. [Record > + Prominence](record.md#what-to-record) then captures the disc and a longer exposure together.

## Hide controls

![Hide controls](../img/manual/hide-controls.png)

**Hide controls** (top right) hides the header, the panel and the tabs so only the live view and its tools remain. The status line stays at the bottom (a lost Sun or a message you have not dismissed must not be hidden), and while a recording runs a red **Recording · Stop** button stays too. **Show controls** brings everything back. Rotate the phone to landscape for the largest picture, and pinch to zoom.

## The status bar

One line at the bottom of the screen, always there, so a message appearing never moves the controls. It shows the last problem (camera error, lost target, storage unavailable, mount error, lost mount link), a lost target ("guiding is paused and resumes by itself", or "guiding goes on from the measured drift" while the Sun is hidden), or what the meridian flip is doing. Tap a long message to read all of it. When there is nothing to say it reads *Ready*. See [Troubleshooting](troubleshooting.md).

## Header: Files, theme and Hide controls

**Files** opens the [file manager](record.md#captures-file-manager) from any tab. The sun or moon button switches between the night theme (dark, easy on dark-adapted eyes) and a **daylight theme** (light, high contrast, for bright sun on a phone screen); the image stays black in both. **Hide controls** shows the image only.

## Quick capture

On every tab except Aim and Record, below the picture, **▶ Start** starts the recording exactly as set up on the [Record tab](record.md) (sequence, prominence or HDR lanes; the button says which, and how many frames) and **Snap** saves a single frame. While recording, the strip shows progress and a Stop button.

## Under the picture

- **Start** is always there with one line saying what it keeps (*Keep video*, *Video + stack*, *Stack only*, *Best frames + stack*, or *Stacker off, saves video*). It is greyed with its reason while the mount calibrates.
- Warnings that matter stay as their own chips (clipping, No Sun found, Mount parked, Calibrating). Information only (the profile, a planet not found yet, a mount clock that differs) is folded into one **N notes** chip that opens a short sheet.
- On a phone the picture is smaller on the tabs that need the room for the panel (Aim, Record, More), and its tools fold behind one **Picture tools** button there. In landscape the tools sit beside the picture, never on it.
- **Set up 2 of 4** (Camera and Aim): the steps still to do (profile, optical setup, site, mount time), each a tap to where it is done. Hide it with the cross.
