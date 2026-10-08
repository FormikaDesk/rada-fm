#!/usr/bin/env python3
"""Copy a 1.5 GB file with vela and watch the progress bar (byte-based) on screen.
Expects ~/inventore/vela/.scratch/{src/random_1500M.bin,dst/}."""
import os, re, subprocess, time
H = os.path.expanduser("~/inventore/vela")
S = f"{H}/.scratch"
X = f"{S}/xdg"
def sh(*a): return subprocess.run(list(a), capture_output=True, text=True)
def snap(): return sh("tmux", "capture-pane", "-t", "cb", "-p").stdout
def keys(*k):
    for x in k:
        sh("tmux", "send-keys", "-t", "cb", x); time.sleep(0.25)
for run in (1, 2):
    sh("tmux", "kill-session", "-t", "cb")
    for f in os.listdir(f"{S}/dst"): os.remove(f"{S}/dst/{f}")
    sh("tmux", "new-session", "-d", "-s", "cb", "-x", "160", "-y", "50", "-c", f"{S}/src",
       f"env XDG_CONFIG_HOME={X}/config XDG_DATA_HOME={X}/data XDG_STATE_HOME={X}/state XDG_CACHE_HOME={X}/cache TERM=xterm-256color COLORTERM=truecolor {H}/target/release/vela; sleep 60")
    time.sleep(1)
    keys("y", "h", "k", "k", "l", "p")
    time.sleep(1.0)
    print(f"run {run}: plan window:", re.sub(r"\s+", " ", " ".join(l[20:130] for l in snap().splitlines()[12:20])).strip()[:200])
    t0 = time.perf_counter(); sh("tmux", "send-keys", "-t", "cb", "Enter")
    seen = []; last = None; done = None
    pid = None
    while time.perf_counter() - t0 < 60:
        o = snap()
        m = re.findall(r"\s(\d+)%\s", o)
        if m and m[-1] != last:
            last = m[-1]; seen.append((time.perf_counter() - t0, int(last)))
        if "done" in o.splitlines()[-1] and os.path.exists(f"{S}/dst/random_1500M.bin"):
            done = time.perf_counter() - t0; break
        time.sleep(0.02)
    size = os.path.getsize(f"{S}/dst/random_1500M.bin") if os.path.exists(f"{S}/dst/random_1500M.bin") else -1
    cmp = subprocess.run(["cmp", f"{S}/src/random_1500M.bin", f"{S}/dst/random_1500M.bin"], capture_output=True).returncode
    print(f"run {run}: finished in {done and round(done,2)} s; dest size {size}; identical={cmp==0}; distinct % readings: {len(seen)}")
    print("   samples:", [(round(t, 2), p) for t, p in seen[:: max(1, len(seen)//10)]])
    sh("tmux", "kill-session", "-t", "cb")
