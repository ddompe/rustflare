---
name: Bug report
about: Something misbehaves
labels: bug
---

**What happened, and what you expected**

**Equipment** (camera, mount and how it is connected, Pi model, RustFlare version from `dpkg -s rustflare`)

**Logs** (please attach what you can; none of it contains your location)
- `GET /api/debug/events?n=300`
- `GET /api/debug/mount?n=300&skip_polls=1` if the mount is involved
- a screenshot of the status bar and the Track tab if tracking is involved
