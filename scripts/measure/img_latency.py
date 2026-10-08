#!/usr/bin/env python3
"""Key-to-screen latency with the cursor moving over a folder full of images.

Same method as bench.py (tmux, 200 alternating j/k keys 30 ms apart, wait until the
on-screen position counter changes). Runs vela twice on the same folder: image previews
off, and image previews on (half blocks: tmux does not pass graphics protocols through).
Expects ~/inventore/vela/.scratch/imgfolder (see the generator in the commit message).
"""
import os, re, statistics, subprocess, sys, time
H = os.path.expanduser("~/inventore/vela"); S = f"{H}/.scratch"; X = f"{S}/xdg"
FOLDER = f"{S}/imgfolder"
def sh(*a): return subprocess.run(list(a), capture_output=True, text=True)
def snap(): return sh("tmux", "capture-pane", "-t", "il", "-p").stdout
def counter(t):
    m = re.findall(r"(\d+)/(\d+) items", t)
    return m[-1][0] if m else None
def run(mode, n=200):
    sh("tmux", "kill-session", "-t", "il")
    env = f"XDG_CONFIG_HOME={X}/config XDG_DATA_HOME={X}/data XDG_STATE_HOME={X}/state XDG_CACHE_HOME={X}/cache TERM=xterm-256color COLORTERM=truecolor"
    sh("tmux", "new-session", "-d", "-s", "il", "-x", "160", "-y", "50", "-c", FOLDER, f"env {env} {H}/target/release/vela --images {mode}; sleep 60")
    t0 = time.perf_counter()
    while "img_000" not in snap() and time.perf_counter() - t0 < 20: pass
    time.sleep(1.0)
    pane = sh("tmux", "list-panes", "-t", "il", "-F", "#{pane_pid}").stdout.split()[0]
    pid = None
    for c in sh("pgrep", "-P", pane).stdout.split() + [pane]:
        for k in [c] + sh("pgrep", "-P", c).stdout.split():
            try:
                if os.path.basename(os.readlink(f"/proc/{k}/exe")) == "vela": pid = int(k)
            except OSError: pass
    lat = []
    # cursor on the 4000x3000 JPEGs at the end of the folder: the heaviest decodes
    sh("tmux", "send-keys", "-t", "il", "G")
    time.sleep(0.8)
    for i in range(n):
        before = counter(snap())
        t0 = time.perf_counter()
        sh("tmux", "send-keys", "-t", "il", "k" if i % 2 == 0 else "j")
        while counter(snap()) == before and time.perf_counter() - t0 < 5: pass
        lat.append((time.perf_counter() - t0) * 1000)
        time.sleep(0.03)
    # how long until the picture itself shows up once the cursor stops
    t0 = time.perf_counter(); seen = None
    sh("tmux", "send-keys", "-t", "il", "g"); time.sleep(0.05)
    while time.perf_counter() - t0 < 5:
        t = snap()
        # "off": the facts line is all there is; otherwise wait for the picture's blocks
        if re.search(r"JPEG · 1600 × 1200 px", t) and (mode == "off" or re.search(r"[▀▄]{6}", t)):
            seen = (time.perf_counter() - t0) * 1000; break
    s = sorted(lat)
    rss = [l for l in open(f"/proc/{pid}/status") if l.startswith(("VmRSS", "VmHWM"))]
    print(f"images {mode:10s} n={n} p50={statistics.median(lat):.0f} p90={s[int(n*.9)]:.0f} p99={s[int(n*.99)]:.0f} max={s[-1]:.0f} ms | "
          f"picture/facts visible {('%.0f ms' % seen) if seen else 'n/a'} after stopping | {' '.join(x.split()[0]+x.split()[1]+'kB' for x in rss)}", flush=True)
    sh("tmux", "kill-session", "-t", "il")
for mode in (sys.argv[1:] or ["off", "halfblocks"]):
    run(mode)
