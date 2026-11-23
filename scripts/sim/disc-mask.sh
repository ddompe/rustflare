#!/bin/bash
# Prominence lane with the coronagraph: the recorded frames stay raw (disc not blacked out), the disc position of every frame is in the
# notes file, and the stacker finds the disc in the stack and draws the black disc after stacking, only if "disc_mask" is on.
R="$(cd "$(dirname "$0")/../.." && pwd)"; T="${TMPDIR:-/tmp}/rustflare-sim/disc-mask"; rm -rf "$T"; mkdir -p "$T/cfg" "$T/data"
S="$R/target/release/rustflare-stack"
"$R/target/release/rustflare-server" --bind 127.0.0.1:18125 --web-dir "$R/web/dist" --data-dir "$T/data" --config-file "$T/cfg/c.json" >"$T/log" 2>&1 & PID=$!
SP=""; trap 'kill $PID $SP 2>/dev/null' EXIT; sleep 1.5
A=http://127.0.0.1:18125/api; J='content-type: application/json'; FAIL=0
post(){ curl -s -X "${3:-POST}" -H "$J" -d "${2:-{\}}" "$A$1"; }
ok(){ if [ "$2" = "1" ]; then echo "ok   $1"; else echo "FAIL $1"; FAIL=1; fi; }
st(){ curl -s $A/status | python3 -c "import sys,json; d=json.load(sys.stdin); print($1)" 2>/dev/null; }
wait_idle(){ for i in $(seq 1 90); do sleep 1; [ "$(st "d['capture']['state']")" = idle ] && [ "$(st "d['after_burst']['stacking']")" = 0 ] && return; done; }
post /camera/open '{"driver":"simulator","id":"sim0"}' >/dev/null; sleep 2
"$S" watch --state-dir "$T/cfg" >"$T/stacker.log" 2>&1 & SP=$!
for i in $(seq 1 15); do [ "$(st "d['after_burst']['stacker_connected']")" = True ] && break; sleep 1; done
ok "the stacker connects" "$([ "$(st "d['after_burst']['stacker_connected']")" = True ] && echo 1)"
for i in $(seq 1 10); do [ "$(st "d['sun'] is not None")" = True ] && break; sleep 1; done
ok "the Sun is in view" "$([ "$(st "d['sun'] is not None")" = True ] && echo 1)"

record(){ # $1 name
  post /capture/after-burst '{"mode":"video_stack","stack_keep_percent":50}' PUT >/dev/null
  post /capture/lanes "{\"name\":\"$1\",\"kind\":\"prominence\",\"lanes\":[{\"name\":\"disc\",\"exposure_us\":2000,\"frames\":16,\"mask_disc\":false},{\"name\":\"prominence\",\"exposure_us\":8000,\"frames\":16,\"mask_disc\":true}],\"cycles\":1,\"format\":\"ser\"}" >/dev/null
  sleep 3; wait_idle; sleep 2
}
# python helper: header keys and a pixel of a FITS stack, and a pixel of the middle frame of a SER
info(){ python3 - "$@" <<'PY'
import sys,struct,json,glob
kind,path=sys.argv[1],sys.argv[2]
if kind=='fits':
    b=open(path,'rb').read(); hdr={}; i=0
    while True:
        blk=b[i:i+2880]; i+=2880
        for k in range(0,2880,80):
            c=blk[k:k+80].decode('latin1')
            if c.startswith('END'): break
            if '=' in c[:10]: hdr[c[:8].strip()]=c[10:].split('/')[0].strip()
        else: continue
        break
    w,h=int(hdr['NAXIS1']),int(hdr['NAXIS2']); bp=int(hdr['BITPIX'])
    cx,cy=int(float(hdr.get('DISCX',w/2))),int(float(hdr.get('DISCY',h/2)))
    # FITS rows run bottom to top
    row=h-1-cy; fmt='>f' if bp==-32 else '>H'; sz=abs(bp)//8
    off=i+(row*w+cx)*sz; px=struct.unpack(fmt,b[off:off+sz])[0]
    print(json.dumps({'DISCX':hdr.get('DISCX'),'DISCMASK':hdr.get('DISCMASK'),'DISCHOW':hdr.get('DISCHOW'),'px':px}))
else:
    h=open(path,'rb').read(); n=struct.unpack('<I',h[38:42])[0]; w,hh,d=struct.unpack('<3I',h[26:38]); bpp=2 if d>8 else 1
    fr=w*hh*bpp; mid=178+(n//2)*fr+((hh//2)*w+w//2)*bpp
    print(h[mid] if bpp==1 else h[mid]+256*h[mid+1])
PY
}
# the stacker is told to draw the disc (the default)
record dm1
SER=$(ls $T/data/*/prominence_*dm1*/lane-prominence/*.ser | head -1); NOTE="${SER%.ser}.json"
ok "the prominence lane frames are raw: the middle of the disc is not black" "$([ "$(info ser "$SER")" -gt 0 ] && echo 1)"
ok "the notes file holds one disc position per frame" "$([ "$(python3 -c "import json; d=json.load(open('$NOTE'))['details']['disc_track']; print(len(d) if all(p[2]>0 for p in d) else -1)")" = 16 ] && echo 1)"
F1=$(ls $T/data/stacks/*/prominence_*dm1*/lane-prominence/stack_*.fits | head -1); I1=$(info fits "$F1")
ok "the stack records where the disc is, fitted to its edge" "$(echo "$I1" | grep -q '"DISCHOW": "'"'"'fitted' && echo 1)"
ok "the stack has the black disc drawn" "$(echo "$I1" | python3 -c "import sys,json; d=json.load(sys.stdin); print(1 if d['DISCMASK']=='T' and d['px']==0 else 0)")"
FD=$(ls $T/data/stacks/*/prominence_*dm1*/lane-disc/stack_*.fits | head -1)
ok "the disc lane is not touched" "$(info fits "$FD" | grep -q DISCX.*null && echo 1)"

# the same with the black disc switched off in the stacker's settings
post /stack/config '{"disc_mask":false}' PUT >/dev/null
record dm2
F2=$(ls $T/data/stacks/*/prominence_*dm2*/lane-prominence/stack_*.fits | head -1); I2=$(info fits "$F2")
ok "with disc_mask off the disc is located but not drawn" "$(echo "$I2" | python3 -c "import sys,json; d=json.load(sys.stdin); print(1 if d['DISCMASK']=='F' and d['px']>0 and d['DISCX'] else 0)")"
# change the setting and make the stack again from its recording
post /stack/config '{"disc_mask":true}' PUT >/dev/null
REL=${F2#$T/data/}
RS=$(post /stack/restack "{\"path\":\"$REL\",\"confirm\":true}")
ok "restack deletes the stack and queues it" "$(echo "$RS" | grep -q '"queued":1' && [ ! -f "$F2" ] && echo 1)"
for i in $(seq 1 40); do [ -f "$F2" ] && break; sleep 1; done
sleep 1; I3=$(info fits "$F2")
ok "the stack is made again with the new setting" "$(echo "$I3" | python3 -c "import sys,json; d=json.load(sys.stdin); print(1 if d['DISCMASK']=='T' and d['px']==0 else 0)")"
echo "$I1"; echo "$I2"; echo "$I3"
[ $FAIL = 0 ] && echo "ALL OK" || { echo "FAILED"; tail -5 "$T/log" "$T/stacker.log"; exit 1; }
