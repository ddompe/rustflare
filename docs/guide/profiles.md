# What are you imaging? (profiles)

RustFlare began as a solar capture server, so most of its words and tools assume the Sun. The **profile** tells it what you are imaging, so the words fit and the tools that make no sense for the target are switched off.

Choose it at the top of the **Camera** tab (*What are you imaging?*):

| Profile | For | What changes |
|---|---|---|
| **Sun, H-alpha** (default) | the Sun through an H-alpha telescope | nothing: the application as it was |
| **Sun, white light** | the Sun with a white-light filter | the same tools |
| **Moon** | the Moon, whole or in part | "Moon" in the words; Prominence view and the prominence recording are greyed (they black out the Sun's disc); the tracker explains that it fits the Moon's bright edge, not the terminator; FITS files say `OBJECT = Moon` |
| **Planet** | a planet (a small bright disc) | also greyed: the disc-edge and full-disc trackers (a planet has no edge to fit) and HDR lanes; the edge circle; white balance Auto is explained as turning a planet grey; FITS says `OBJECT = Planet` |
| **Other** | anything else | nothing assumed; FITS has no `OBJECT` |

What does not apply is **greyed, not hidden**, with the reason written under the control (a phone has no hover). The page lists the same controls in every profile, and the HTTP API refuses nothing: the profile adapts the screen and the headers, it is not a lock. A status-bar chip names the profile whenever it is not a Sun one.

The profile is never changed by itself. **Go to** the Moon (or a planet, or the Sun) while another profile is chosen shows a line with a button to switch.

Things that apply to every profile and are explained where they happen: the Pi's stacker handles mono recordings only, so with a colour camera the stacking modes under *After each burst* are greyed (record the raw colour video and stack it in AutoStakkert, PIPP or Siril); the **automatic meridian flip** and the flip assist follow the Sun, so they are greyed for the Moon and planets (flip by hand with Go to after the meridian).

## Moon or planets with a colour camera

Use **Colour** (Camera tab) set to **Manual** or **Off** for a planet, because Auto makes the bright part neutral. Use *Frame size* to crop to the planet for frame rate and record **SER video**: the file is the raw mosaic, which AutoStakkert, PIPP and Siril debayer and stack. The live view is a half-resolution preview.

What is not there yet, from the [proposal](../project/proposal-use-profiles.md): a planet tracker (nothing marks the planet in the picture, hold it with the slew pad or a guide star), the Moon's and planets' own expected sizes, a peak-based auto-stretch for a small bright disc, and per-channel levels. Recording defaults (frames, seconds) are not yet per profile.
