# What are you imaging? (profiles)

RustFlare began as a solar imaging server, so most of its words and tools assume the Sun. The **profile** tells it what you are imaging, so the words fit and the tools that make no sense for the target are switched off.

Choose it at the top of the **Camera** tab (*What are you imaging?*, one compact list; **What is this?** shows the longer sentence). The profile never changes the mount's tracking rate: on **Aim**, when the profile's natural rate (Lunar for the Moon, Star for planets) differs from the mount's, one tap offers to switch it. Choosing a rate there switches tracking on at that rate.

| Profile | For | What changes |
|---|---|---|
| **Sun, H-alpha** (default) | the Sun through an H-alpha telescope | nothing: the application as it was |
| **Sun, white light** | the Sun with a white-light filter | the same tools |
| **Moon** | the Moon, whole or in part | "Moon" in the words; Prominence view and the prominence recording are greyed (they black out the Sun's disc); the tracker explains that it fits the Moon's bright edge, not the terminator; the expected size is the Moon's from its distance; the meridian flip follows the Moon; an unnamed recording is called `Moon`; FITS files say `OBJECT = Moon` |
| **Planet** | a planet (a small bright disc) | also greyed: the disc-edge and full-disc trackers (a planet has no edge to fit) and HDR lanes; the edge circle; white balance Auto is explained as turning a planet grey. **Planet centroid** tracking is on (and the default), the engine marks the planet in the picture, clipping is judged by the brightest part of the picture and auto-stretch is set by it, the flip follows the planet last gone to; FITS says `OBJECT = Planet` |
| **Other** | anything else | nothing assumed; the Sun detector does not run (no outline, no "No Sun found"); the automatic flip is greyed (it has no body to follow); FITS has no `OBJECT` |

What does not apply is **greyed, not hidden**, with the reason written under the control (a phone has no hover). The page lists the same controls in every profile, and the HTTP API refuses nothing: the profile adapts the screen and the headers, it is not a lock. The profile in use is named in the line under the picker on the Camera tab (with the optical setup in use), and the status API reports it as `status.profile` and the target as `status.target`.

The profile is never changed by itself. **Go to** the Moon (or a planet, or the Sun) while another profile is chosen shows a line with a button to switch.

Things that apply to every profile and are explained where they happen: the Pi's stacker handles mono recordings only, so with a colour camera the stacking modes under *After each burst* are greyed (record the raw colour video and stack it in AutoStakkert, PIPP or Siril).

## The meridian flip follows the target

The **automatic meridian flip** and the **flip assist** follow what the profile images: the Sun on the Sun profiles, the Moon on the Moon profile (it crosses the meridian about 50 minutes later each day, and the time shown is computed at the Moon's own rate), the planet you last used **Go to** on for the Planet profile (until you have gone to one, the flip is greyed and says so). After the mount has turned over, the tracking rate is put back to the target's (solar, lunar or sidereal), not always solar. The recorded position names its target, so *Restore after a manual flip* keeps working if you change the profile in between. Re-centring needs the target in the picture (the Moon disc, or the planet found by the planet locator) and a calibration. The image still turns by about 180 degrees after a flip: recalibrate if you were guiding.

## A telescope carries its profile

A saved [optical setup](more.md#optical-setup-your-telescopes) (a telescope with its reducer or Barlow) carries the profile it is used for: *Lunt LS50 + Barlow* with Sun, H-alpha, *C11 + reducer* with Moon. Applying the setup switches the profile along with the focal length, so changing telescope is one step. It still never moves the mount or starts anything, and you can change the profile afterwards by hand: the setup then shows as *modified* until you revert it (or save the new values over it with Edit in Manage...). A setup saved with *Keep the current profile* leaves the profile alone.

## Moon or planets with a colour camera

Use **Colour** (Camera tab) set to **Manual** or **Off** for a planet, because Auto makes the bright part neutral. Use *Frame size* to crop to the planet for frame rate and record **SER video**: the file is the raw mosaic, which AutoStakkert, PIPP and Siril debayer and stack. The live view is a half-resolution preview.

### Tracking a planet

With the Planet profile the engine looks for the brightest small disc on a dark sky, marks it, and **Planet centroid** (Track tab) follows its centre. It needs the planet to stand out clearly (about 8 times the sky noise and half a percent of full scale above the sky) and to be at least a few pixels across; a star or a hot pixel is rejected, and a disc larger than a tenth of the frame is not a planet (use Disc edge or Feature). Seeing moves a small planet by several pixels, so the one model of where the target is accepts steps of up to 8 pixels for this profile, however small the planet. Put the planet in the frame with the slew pad first; the tracker only searches near where the planet last was, so a bright moon or star elsewhere cannot take over.

For the planet, *clipping* means the brightest part of the picture reaches full scale (the mean and the saturated fraction cannot see a planet that is a tenth of a percent of the frame), and the auto-stretch is set by that brightest part so the planet is not blown out.

### The Moon with a phase

The Moon's bright edge is found well when it is nearly full. With a terminator the detector takes the **lit part** for the disc: the marked centre moves toward the lit side and the circle is smaller than the Moon. That is why the Moon profile explains that the terminator is not used. Hold a crescent or quarter Moon by hand or with **Feature** on a crater.

What is not there yet, from the [proposal](../project/proposal-use-profiles.md): per-channel levels, a planet focus metric, the dead band in arcseconds, and recording defaults (frames, seconds) per profile. Nothing in this section has been tried on a real Moon or planet yet, only on the simulators.
