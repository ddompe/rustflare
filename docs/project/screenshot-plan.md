# Plan: README and manual screenshots from a real session

!!! note "Status"
    Not done. The manual pictures in `docs/img/manual/` are still the simulator pictures of 2026-11-21 and the README has one real picture (`docs/img/real/disc-and-prominence.jpg`). The screen has changed since the plan was written (the Tools sheet, the notes chip, the *Set up N of 5* strip, optical setups, the cooler section), so the list of states to prepare needs a pass before the day, and the pictures should be taken with 0.8.2 or later.

Goal: someone who lands on the GitHub page and reads nothing but the pictures should understand what RustFlare does: **a phone in front of a telescope, the Sun on the screen, one button to record, and the results on the same page.** The details stay in the manual.

Do it on the next clear day with the Sun in view, after 0.6.4 (or later) is installed. One session, about 30 minutes of Sun time; most of the work is getting the states in order, the script then takes every picture in a minute.

## The pictures (in README order)

| # | File | What it shows | State needed on the real system |
|---|---|---|---|
| 1 | `hero-phone.webp` | A phone (portrait) on the **Aim** tab: live Sun with the outline and tracking circle, the offset readout, **Start** under the picture. The one picture that says what the product is | Camera open, Sun in view and tracked, mount connected |
| 2 | `hold.webp` | **Track** tab, calibrated and holding, with the `locked` word and a small error (a minute after Hold position, when it has settled) | Calibrated, Hold position on, locked |
| 3 | `record.webp` | **Record** tab with *After each burst* open: the four modes, the plan line (size, time, "keeps up") | Camera open; set *Video and stack* with the stacker running so the line says it keeps up |
| 4 | `stacks.webp` | The file manager in `stacks/`: the thumbnail grid of real stacks (discs and one prominence) | At least 6 finished stacks of different recordings; include one prominence stack |
| 5 | `prominence.webp` | The existing `docs/img/real/disc-and-prominence.jpg` stays as the hero of the page; this one is the live view in **prominence view** with the black disc and a real prominence | Prominence view on, exposure for prominences |
| 6 | `landscape.webp` | The phone in landscape on the tripod (live view with the overlay widgets) | Overlays on: fps, guiding graph, histogram |
| 7 | `desktop.webp` | Desktop browser, live view with widgets and the tab column: for the "also works on a laptop" line | Same as 6 |
| 8 | `device-cards.webp` | The camera and the mount cards expanded, with their photos | Real camera and mount connected; the rows under *This unit* (serial) stay collapsed |

Keep 1 to 4 in the README body as a row or a 2x2 table (each about 300 px wide, linking to the full picture), 5 to 8 only in the manual. A short caption under each, one line.

Real pictures beat simulator pictures: the simulator Sun is a grey disc. The existing simulator screenshots in `docs/img/manual/` are replaced by real ones where a real one exists, and stay where one cannot (Meridian flip, error states).

## How the pictures are taken

- `scripts/screenshots.cjs <url> <out-dir>` (already written and tried on the simulators). It only looks: it taps tabs and opens disclosures and never records, slews or changes a setting. It takes the phone pictures at 390 x 844 with 3x density (an iPhone 14 Pro profile), the landscape picture, and the desktop ones at 1440 x 900 with 2x. `--only=phone-aim,phone-track` retakes a few.
- Run it from the Mac against the Pi (`https://astropi.local:8443`; the page must be reachable), so the pictures are of the real app and the real image, and nothing needs a phone in hand. For a true iPhone picture (status bar, Home Screen app) use the phone's own screenshot for #1 only and compare.
- Colour of the Sun: the live view is grayscale by default. For the README use the **H-alpha orange** tint (More > Display) so the pictures match the hero.
- The time of day matters for the light on the tripod; for the pictures themselves it does not.

## Privacy: checked before anything is committed

The script never opens More > Site or the *This unit* rows, and blurs text that looks like a coordinate, a serial number, an IP or MAC address or a `.local` host name (it says when it did). That is a second guard, not the first: **look at every picture** before it goes into git. Things that have leaked before or could: the site latitude and longitude, camera and mount serial numbers, Wi-Fi names, the Pi's host name in the address bar (desktop pictures are cropped to the page, no browser chrome), file names that contain a place or a person.

## Preparing the state, in this order

1. Install the release, open the camera and the mount, Go to Sun, switch the tint to orange.
2. Check the stacker is running and the stick is on USB 2 (the page warns otherwise).
3. Record two short bursts of video and stack (so the file manager has stacks), and one prominence recording (two cycles) for the prominence stack.
4. Calibrate, Hold position, wait one minute until it says *locked*.
5. Take the pictures (`node scripts/screenshots.cjs ...`), then retake any that show a warning, a red chip, a dark frame or clouds.
6. In the file manager, check the stacks grid has no personal file names; rename the recordings first if it has.

## Processing and committing

- Convert to WebP: `cwebp -q 88 -m 6 in.png -o out.webp` (the phone pictures at 1170 px wide, the desktop ones at 1440). About 60 to 150 KB each; the page should stay under 1 MB of images.
- Put them in `docs/img/real/` and `docs/img/manual/` (the manual ones replace the old files with the same name so the manual pages need no change), and add `width` and `alt` to every `<img>`. Dark and light mode: the pictures are dark, which looks right on both.
- README: after the hero photo, one line (what it is) and a row of #1 to #4 as a table with the captions, then the existing Features list. The manual already uses `docs/img/manual/*.png`; refresh those from the same run.
- Commit with a note in `docs/reference/device-images.md` style if a vendor photo or a third party appears in any picture (the device cards show manufacturer photos).

## Checklist for the day

- [ ] 0.6.4 or later installed on the Pi, stacker active, stick on USB 2
- [ ] orange tint, overlays on (fps, guiding graph, histogram), prominence exposure set
- [ ] 6 or more stacks, one of them prominence
- [ ] calibrated, holding, *locked*
- [ ] `node scripts/screenshots.cjs https://astropi.local:8443 ./shots` (from `web/`)
- [ ] every picture looked at: no coordinates, serials, Wi-Fi names, clouds, red chips
- [ ] converted, committed, README updated, manual images refreshed, CI green
