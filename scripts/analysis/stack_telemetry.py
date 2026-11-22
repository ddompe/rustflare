#!/usr/bin/env python3
"""Summarise the stacker's telemetry (stack.jsonl): the limits it was given, every job (time, CPU used, throttling, temperature, what the
recorder was doing), the pauses, and anything that looks like interference with a recording.

  scripts/analysis/stack_telemetry.py /var/lib/rustflare/logs                 # a logs folder (day folders inside) or one stack.jsonl
  scripts/analysis/stack_telemetry.py http://astropi.local:8080               # through the server's debug API
"""
import json, sys, glob, os, urllib.request, ssl

def lines(src):
    if src.startswith("http"):
        ctx = ssl._create_unverified_context()
        listing = json.load(urllib.request.urlopen(src.rstrip("/") + "/api/debug/logs", context=ctx))
        files = [f for f in (listing.get("files") or listing) if str(f.get("path", f.get("name", ""))).endswith("stack.jsonl")]
        for f in sorted(files, key=lambda f: f.get("path", f.get("name", ""))):
            p = f.get("path", f.get("name"))
            base = "/api/debug/internal/" if f.get("where", "internal") == "internal" else "/api/debug/drive/"
            for l in urllib.request.urlopen(src.rstrip("/") + base + p, context=ctx).read().decode().splitlines():
                yield json.loads(l)
        return
    paths = [src] if src.endswith(".jsonl") else sorted(glob.glob(os.path.join(src, "*", "stack.jsonl")) + glob.glob(os.path.join(src, "stack.jsonl")))
    for p in paths:
        for l in open(p):
            if l.strip():
                yield json.loads(l)

def main():
    ev = list(lines(sys.argv[1] if len(sys.argv) > 1 else "/var/lib/rustflare/logs"))
    if not ev:
        print("no telemetry found"); return
    for e in [e for e in ev if e["kind"] == "start"][-1:]:
        env, unit = e.get("environment", {}), e.get("unit") or {}
        cg = env.get("cgroup", {})
        print(f"stacker {e.get('version')} started {e['iso']}Z")
        print(f"  cpu.max {cg.get('cpu.max')}  cpu.weight {cg.get('cpu.weight')}  io.weight {cg.get('io.weight')}  memory.max {cg.get('memory.max')}")
        print(f"  cgroup controllers: {env.get('cgroup_controllers')}" + ("   <- no memory controller: MemoryMax is NOT enforced" if 'memory' not in (env.get('cgroup_controllers') or 'memory') else ''))
        print(f"  cpus allowed {env.get('cpus_allowed')} of {env.get('cores')}  nice {env.get('nice')}  user {unit.get('User')}  io class {unit.get('IOSchedulingClass')}  kernel {env.get('kernel')}")
    jobs = [e for e in ev if e["kind"] == "job"]
    print(f"\n{len(jobs)} jobs, {sum(1 for j in jobs if not j.get('ok'))} failed")
    if jobs:
        print(f"{'time (UTC)':20} {'source':34} {'wall':>6} {'cores':>5} {'throt s':>7} {'tmax':>5} {'fmin':>5} {'rec busy':>8}  recorder states")
        for j in jobs:
            d = j.get("during", {})
            print(f"{j['iso'][:19]:20} {j['source'][-34:]:34} {j['wall_s']:6.1f} {j['cpu_cores_used']:5.2f} {j['cgroup_throttled_s']:7.2f} {str(d.get('temp_c_max'))[:5]:>5} {str(d.get('freq_mhz_min'))[:5]:>5} "
                  f"{d.get('recorder_busy_samples', 0):>3}/{d.get('samples', 0):<4}  {','.join(d.get('recorder_states', []))}")
        walls = sorted(j["wall_s"] for j in jobs)
        print(f"\nwall time: median {walls[len(walls)//2]:.1f} s, max {walls[-1]:.1f} s; cpu {sum(j['cpu_user_s'] + j['cpu_sys_s'] for j in jobs):.0f} s in total")
        phases = {}
        for j in jobs:
            for name, s in j.get("phases", []):
                phases.setdefault(name, []).append(s)
        for name, v in phases.items():
            print(f"  {name[:60]:60} median {sorted(v)[len(v)//2]:6.2f} s")
        irq = {}
        for j in jobs:
            for k, v in (j.get('irq_per_core') or {}).items():
                irq[k] = [a + b for a, b in zip(irq.get(k, [0] * len(v)), v)]
        if irq:
            print('\ninterrupts during jobs, per core:')
            for k, v in sorted(irq.items(), key=lambda kv: -sum(kv[1]))[:5]:
                print(f'  {k:28} ' + '  '.join(f'cpu{i}:{int(x)}' for i, x in enumerate(v)))
        busy = [j for j in jobs if j.get("during", {}).get("recorder_busy_samples", 0) > 0]
        idle = [j for j in jobs if j.get("during", {}).get("recorder_busy_samples", 0) == 0]
        if busy and idle:
            m = lambda js: sorted(j["wall_s"] for j in js)[len(js)//2]
            print(f"\njobs that overlapped a burst being written: {len(busy)} (median {m(busy):.1f} s) vs {len(idle)} that did not (median {m(idle):.1f} s)")
        print("\nflags:")
        flags = 0
        for j in jobs:
            d, why = j.get("during", {}), []
            if j["cgroup_throttled_periods"] > 0: why.append(f"CPU-throttled {j['cgroup_throttled_s']:.2f} s")
            if (d.get("temp_c_max") or 0) >= 80: why.append(f"{d['temp_c_max']} C")
            if d.get("throttled_flags_seen") and set(d["throttled_flags_seen"]) - {"0x0"}: why.append(f"firmware throttling {d['throttled_flags_seen']}")
            if j.get("psi_stall_s", {}).get("io_full", 0) > 0.5: why.append(f"I/O stall {j['psi_stall_s']['io_full']:.1f} s")
            if j.get("psi_stall_s", {}).get("memory_full", 0) > 0.1: why.append("memory stall")
            if d.get("recorder_busy_samples", 0) and j.get("policy") != "always": why.append("ran while the recorder was writing (policy " + str(j.get("policy")) + ")")
            if why:
                flags += 1; print(f"  {j['iso'][:19]} {j['source'][-40:]}: " + "; ".join(why))
        if not flags: print("  none")
    states = [e for e in ev if e["kind"] == "state"]
    if states:
        print("\nstate changes:")
        for a, b in zip(states, states[1:] + [None]):
            dur = (b["t"] - a["t"]) if b else None
            r = a.get("recorder") or {}
            print(f"  {a['iso'][11:19]} {a['state']:9} {a['reason'][:48]:48} pending {a['pending']:<3} recorder {r.get('state')}/{'busy' if r.get('busy') else 'free'}" + (f"  for {dur:.0f} s" if dur is not None else ""))

main()
