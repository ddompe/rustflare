# Performance

Numbers measured on a Raspberry Pi 5 (4 cores) with a Player One Apollo-M MINI and a USB 3 stick. Your figures depend on the camera, the USB port and the drive.

## Camera frame rate

| Camera and mode | Frame rate |
|---|---|
| Apollo-M MINI, full frame 1944x1472, 16-bit | 57 fps (about 326 MB/s) |
| Same, 8-bit | 82 fps |
| Same, 50 % frame size (968x736) | 153 fps |
| Same, 25 % frame size (480x368) | 153 fps |
| ASI174MM Mini on a USB 2 port, full frame, 16-bit / 8-bit | 8.7 / 17.3 fps |

The full frame is limited by the USB 3 link. At 50 % and 25 % the rate is limited by the exposure, so a shorter exposure raises it. The ASI174MM figure is the USB 2 limit; a USB 3 port and cable are needed for its higher rates and have not yet been measured. Frame size is set in the [Camera tab](../guide/camera.md#frame-size) (Player One and the simulator; not yet available for ZWO ASI).

## Storage speed

- SD card (Pi 5): about 65 MB/s sustained, about 11 fps of full-frame 16-bit video.
- USB 3 stick: 75 to 90 MB/s sustained.

The RAM buffer is what makes faster bursts possible: frames are captured at camera speed and written in the background. See [Capture formats](capture-formats.md#bursts-and-the-ram-buffer).

## CPU load

At full rate (57 fps, 1944x1472) the whole program uses about a third of one of the four cores with nobody watching, and about 46 % with one phone showing the live view. The preview is only produced while someone is watching, so an unattended recording pays none of its cost. There is plenty of headroom for recording, tracking and guiding at the same time.

Profiling notes for developers: [Performance audit](../project/performance-audit.md).
