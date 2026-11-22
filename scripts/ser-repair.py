#!/usr/bin/env python3
"""Repair a SER file that was cut short (drive pulled, power lost): the header says 0 frames or too many, and the timestamp
trailer is missing. Counts the whole frames that are really in the file, drops a half-written last frame, patches the frame
count and appends a timestamp trailer (frames are spaced 1/--fps apart from the header's start time, since the real stamps are
gone). Without --write it only reports.

    scripts/ser-repair.py recording.ser              # report
    scripts/ser-repair.py recording.ser --write      # repair in place (works on a copy with --copy)
"""
import argparse, os, shutil, struct, sys

HEADER = 178
parser = argparse.ArgumentParser()
parser.add_argument("file")
parser.add_argument("--write", action="store_true")
parser.add_argument("--copy", action="store_true", help="write NAME.repaired.ser and leave the original alone")
parser.add_argument("--fps", type=float, default=60.0, help="frame rate to space the rebuilt timestamps (default 60)")
a = parser.parse_args()

with open(a.file, "rb") as f:
    h = f.read(HEADER)
if len(h) < HEADER or h[:14] != b"LUCAM-RECORDER":
    sys.exit("not a SER file")
w, hh, depth, declared = struct.unpack("<4I", h[26:42])
frame = w * hh * depth // 8
size = os.path.getsize(a.file)
whole = (size - HEADER) // frame
complete = HEADER + declared * (frame + 8) == size
print(f"{a.file}: {w}x{hh} {depth}-bit, header says {declared} frames, the file holds {whole} whole frames ({size} bytes)")
if complete and declared > 0:
    sys.exit("the file is complete: nothing to repair")
if whole == 0:
    sys.exit("no whole frame in the file")
print(f"-> would keep {whole} frames, drop {size - HEADER - whole * frame} stray bytes, rebuild the timestamps at {a.fps:g} fps")
if not (a.write or a.copy):
    sys.exit(0)
target = a.file
if a.copy:
    target = a.file[:-4] + ".repaired.ser"
    shutil.copyfile(a.file, target)
start = struct.unpack("<Q", h[170:178])[0] or struct.unpack("<Q", h[162:170])[0]
with open(target, "r+b") as f:
    f.truncate(HEADER + whole * frame)
    f.seek(38)
    f.write(struct.pack("<I", whole))
    f.seek(0, os.SEEK_END)
    step = int(1e7 / a.fps)
    f.write(b"".join(struct.pack("<Q", start + i * step) for i in range(whole)))
print(f"repaired {target}: {whole} frames")
