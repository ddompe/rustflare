#!/bin/bash
# Scenario against the simulators (no hardware): needs `cargo build --release -p rustflare-server` and `cd web && npm run build` first.
R="$(cd "$(dirname "$0")/../.." && pwd)"; T="${TMPDIR:-/tmp}/rustflare-sim/mask"; rm -rf "$T"; mkdir -p "$T/data"
"$R/target/release/rustflare-server" --bind 127.0.0.1:18114 --web-dir "$R/web/dist" --data-dir "$T/data" --config-file "$T/c.json" >"$T/log" 2>&1 & PID=$!
trap 'kill $PID 2>/dev/null' EXIT; sleep 1.5
A=http://127.0.0.1:18114/api; J='content-type: application/json'
curl -s -X POST -H "$J" -d '{"driver":"simulator","id":"sim0"}' $A/camera/open >/dev/null; sleep 2
curl -s -X POST -H "$J" -d '{"kind":"prominence","cycles":1,"lanes":[{"name":"disc","exposure_us":2000,"frames":3},{"name":"prominence","exposure_us":2000,"frames":3,"mask_disc":true}]}' $A/capture/lanes; echo
for i in $(seq 1 20); do sleep 1; S=$(curl -s $A/status | python3 -c 'import sys,json; print(json.load(sys.stdin)["capture"]["state"])'); [ "$S" = idle ] && break; done; sleep 1
python3 - <<'PY'
import glob,struct
for f in sorted(glob.glob('$R/.local/masksim/data/*/prominence_*/*/*.ser')):
    d=open(f,'rb').read(); w,h,depth,n=struct.unpack('<4I',d[26:42]); bpp=depth//8
    off=178; px=lambda x,y: int.from_bytes(d[off+(y*w+x)*bpp:off+(y*w+x)*bpp+bpp],'little')
    print(f.split('/')[-1], n, 'centre', px(w//2,h//2), 'corner', px(5,5))
PY
