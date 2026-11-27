# The screen

The **live view** is on top with its overlays. The five **tabs** are at the bottom and the controls of the selected tab sit in between. The tabs follow a session:

1. [Camera](camera.md): open the camera, set exposure and gain, flats and darks.
2. [Aim](aim.md): connect the mount, Go to Sun, meridian flip.
3. [Track](track.md): follow the Sun and correct drift.
4. [Record](record.md): single frames, sequences, bursts, files.
5. [More](more.md): display, optical setup, site, stacking, storage, RAM buffer, session log, startup.

A mark on a tab shows whether that step is ready (a check), needs attention (a warning) or has a problem; a recording shows on the Record tab. See [Under the picture](#under-the-picture).

The page itself never scrolls: the header, the picture and the tab bar stay where they are, and only the controls of the selected tab scroll. Nothing moves when a message appears.

!!! note "Screenshots"
    The pictures on the manual pages (`docs/img/manual/`) were taken on 2026-11-21 and are out of date: the Tools sheet, the notes chip, the *Set up N of 5* strip, the Start strip, the optical setup dropdown, the colour histogram and the cooler section are newer than they are. The text is current.

## Overlays

Overlays sit on the picture, are semi-transparent and can be dragged anywhere. Tap one to shrink it to one line. **Tools > Widgets** switches each one on or off and sets their opacity.

- **Status**: camera fps, screen fps (the preview rate your phone receives), disk MB/s while recording, exposure, gain, sensor temperature, mount state, and whether a flat or dark is applied.
- **Recording** and **RAM buffer and storage**: appear while recording.
- **Guiding error**: the average error and a one-minute graph while guiding.
- **Histogram**.
- **Prominence mask size**: with the Prominence view on, a slider for the size of the black disc (90 to 120 % of the Sun's radius). It starts over the middle of the picture, which the mask blacks out anyway; drag it elsewhere if you like. The same size is used for a prominence lane that records with the coronagraph.
- **Focus score**: with Focus assist on, the score, the best so far and a short history. On a phone it starts as one line; tap it for the graph.
- **Meridian flip**: appears when the automatic flip is on and the Sun is within 15 minutes of the meridian, and while it flips. It says what is going on and whether a recording is paused.

**Tools > Overlays** hides every drawn overlay (crosshair, rings, Sun marks) so you see the bare image. In display settings the Sun edge circle and the centre dot with its arrow are separate switches.

**Tools > Display** opens the display settings (More > Display): auto-stretch, brightness, contrast, gamma, colour tint and bullseye. These change only what you see, never what is recorded.

## Prominence view

**Tools > Prominence** (the Prominence view) blacks out the Sun's disc (display only). Raise exposure or gain and watch the faint structure at the limb appear, so you can judge the exposure you want for prominences. It does not stretch the image, so what you see is the real brightness. Turn on **Stretch** if you want it brightened: it is in the Tools sheet and has a switch in More > Display, and both show the same setting for everyone watching. [Record > + Prominence](record.md#what-to-record) then captures the disc and a longer exposure together.

## Hide controls

![Hide controls](../img/manual/hide-controls.png)

**Hide controls** (top right of the header) hides the header, the panel, the tabs and the Start strip so only the live view and its tools remain. The status bar stays at the bottom (a lost Sun or a message you have not dismissed must not be hidden), and while a recording runs a red **Recording · Stop** button stays too. **Show controls** brings everything back. Rotate the phone to landscape for the largest picture, and pinch to zoom.

## The status bar

One line at the bottom of the screen, always there, so a message appearing never moves the controls. It shows the last problem (camera error, lost target, storage unavailable, mount error, lost mount link), a lost target ("guiding is paused and resumes by itself", or "guiding goes on from the measured drift" while the Sun is hidden), or what the meridian flip is doing. Tap a long message to read all of it. When there is nothing to say it reads *Ready*. At its right edge it shows the software version (for example `v0.8.2`; a build made on a computer adds `local` and the commit it was built from). See [Troubleshooting](troubleshooting.md).

## Header: Files, theme and Hide controls

**Files** opens the [file manager](record.md#files-file-manager) from any tab. The sun or moon button switches between the night theme (dark, easy on dark-adapted eyes) and a **daylight theme** (light, high contrast, for bright sun on a phone screen); the image stays black in both. **Hide controls** shows the image only.

## The Start strip

On every tab, below the picture, while the camera is open and nothing is recording, **Start** starts the recording exactly as set up on the [Record tab](record.md) (sequence, prominence or HDR lanes; the button says which, and how many frames) and **Snap** saves a single frame. While recording, the strip shows progress and a Stop button.

## Under the picture

- **Start** is always there with one line saying what it keeps (*Keep video*, *Video + stack*, *Stack only*, *Best frames + stack*, or *Stacker off, saves video*). It is greyed with its reason while the mount calibrates.
- Warnings that matter stay as their own chips (clipping, No Sun found, Mount parked, Calibrating). Other things worth knowing (a planet not found yet, a mount clock that differs, what the last recording saved) are folded into one chip that leads with the key words (*Clock off 3 min*, or *2 notes: clock, sun*) and is drawn as a warning when any of its notes is one. The chips sit on the picture, top left, so they never change the picture's size when they come and go.
- **Tools** (a button on the picture) opens a sheet with every picture tool named in words: *Zoom in*, *Zoom out*, *Prominence*, *Stretch*, *Overlays*, *Widgets*, *Display*. A tool that does not apply is greyed with its reason in the sheet. On a phone the sheet opens over the panel, not over the picture.
- **Set up 2 of 5**: the five first-session steps in order (open the camera, profile, optical setup, site, mount and its time). Only steps that are really done count: the profile counts once you have chosen it or pressed *Keep*, the mount counts when it is connected and its clock matches (or is sent by itself). On the Camera tab it is one line at the top with the steps still to do (the next one is orange); on the other tabs it is a chip that opens the same list. Hide it with the cross.
- The mark on each tab says whether its job is done and, in its tooltip and for screen readers, why: Camera (open, no clipping), Aim (mount connected, tracking, clock matches; a warning for a parked mount, a lost link, a differing clock or tracking off), Track (a check while locked; a warning for off target, settling for more than a minute or a failed calibration; a problem when the target is lost).
