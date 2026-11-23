# After each burst: video, stacks, or both

Raw video is the product, but a night of 4 GB bursts fills a drive quickly, and many people end up deleting the video after stacking it. **Record > After each burst** lets the recorder stack each burst straight from memory, so the video never has to make the round trip through the disk if you do not want it to.

!!! note "Status"
    Built and tested on the simulators and on a Pi 5 with a real camera for the recording itself. Stacking from memory alongside a real camera on the Sun is still to be verified: see the [hardware checklist](../project/hardware-checklist.md). Until you have compared the stacks with your own, keep the video (mode 1 or 2) for anything you cannot repeat, such as an eclipse.

## The four modes

| Mode | On disk after a burst | Use it when |
|---|---|---|
| **Keep the video** (default) | The full raw video. Stacking, if you want it, happens later from the file (More > Stacking). | You process everything yourself, or it is an eclipse. |
| **Video and stack** | The full video and a stack made from memory. | You want both and have the disk. |
| **Stack only** | One stack (a 32-bit float FITS) and **no video**. About 100 times smaller. | Routine solar sessions where you throw the video away. The raw frames are not kept. |
| **Best frames only** | A stack and a short video of only the best frames. | You want to be able to re-stack the best of each burst yourself, without keeping all of it. |

Two percentages go with these, both configurable: **frames in each stack** (default 25 %: the best quarter of the burst by sharpness) and, for *Best frames only*, **frames kept as video** (default 25 %). They can differ: stack the best 10 % but keep the best 30 % as video, so you can try other settings later.

The plan line under the modes shows what a burst costs on disk, how much that is an hour at your repeat interval, how long a stack takes (measured, not guessed) against the gap between bursts, and whether a burst fits in the RAM buffer.

## What the recorder does

Frames go into a **ring in shared memory** (the RAM buffer in More). The raw-file writer reads them from it, and so does the stacker, which is a separate program. Neither can slow the camera down: if a reader falls behind, the camera simply keeps writing and overwrites the oldest frames. The one exception is the file writer in *Wait for the disk* mode, which holds the camera back so that no frame is lost (see [Capture formats](../reference/capture-formats.md)).

In *Stack only* and *Best frames only*, a burst's frames wait in the ring while the stacker works on them (a few seconds for a 100-frame burst), and are then discarded or partly written. The camera is never held back for the stacker.

## It is never quiet when it cannot do what you asked

- **The stacker is not running** and you choose *Stack only* or *Best frames only*: the choice is refused with the reason, and a recording that would depend on it is not started. With *Video and stack* the recording starts, writes video, and says so.
- **A burst is too big for the RAM buffer** to hold while it is stacked: it is recorded as video, with a notice saying why. Raise the buffer in More > RAM buffer, or shorten the burst.
- **A stack might not finish before the camera overwrites the burst**: the recorder plans each stack at its measured time multiplied by a **safety margin** (default 2, range 1 to 5, under *Safety margin*). If a stack is not going to make it, that burst's video is written instead and a notice says so. A larger margin gives up sooner.
- **The stacker stops or fails** in the middle of a recording, or a stack fails for any reason: every burst waiting for it is written as video, and the rest of the recording is video.

Each of these puts a message in the **status bar** at the bottom, which stays until you press *Dismiss*, and an entry in the event log (`notice`, `stacker_disconnected`, `stack_done`). A fallback that worked is still shown, as a warning: nothing is lost, but it is not what you asked for.

## Where things end up

Stacks go to `stacks/` on the drive, mirroring the folders, and are listed in More > Stacking like any other. Each burst's `.json` says what happened (`stack`: output file, frames kept, seconds, whether the video was written). The disk watcher does not stack again a video whose burst was already stacked from memory.

## Disk and time, roughly

A 1944 x 1472 16-bit burst of 100 frames is 570 MB of video; its stack is 11 MB. At one burst every 30 s that is about 68 GB an hour of video against 1.3 GB an hour of stacks.

## Technical notes

The design and the reasons for it are in [ADR 0019](../adr/0019-shared-frame-ring.md). The recorder and the stacker talk over a socket in the recorder's settings folder (`stack.sock`); the ring itself is passed as a file descriptor, so the stacker reads the camera's frames without a copy. Either side can restart: the stacker reconnects by itself.
