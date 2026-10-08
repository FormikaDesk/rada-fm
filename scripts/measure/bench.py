#!/usr/bin/env python3
"""Startup, key latency and memory of rada (and, if installed, superfile and yazi), measured the same way.

Method: each program runs inside a tmux
session (160x50, TERM=xterm-256color); "startup" is the time from launching the
session until a known file name shows on screen (polling `tmux capture-pane`);
"latency" is the time from sending `j`/`k` until the on-screen position counter
changes (200 alternating keys, 30 ms apart); RSS comes from /proc.

Everything runs against isolated XDG folders under <project>/.scratch/.
The fixture folders are only read. Point RADA_BENCH_FIXTURES at a folder holding
`code/` (14 files, one named binary.dat), `many10k/` (file_00000…file_09999) and
`dirs3k/` (dir_0000…); RADA_BENCH_SPF names the superfile binary (default: `spf` in PATH).

usage: bench.py [startup|latency|all] [rada|spf|yazi ...]
"""
import os
import re
import statistics
import subprocess
import sys
import time

HOME = os.path.expanduser("~")
RADA = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))  # the project root
SCR = f"{RADA}/.scratch"
FIXTURES = os.environ.get("RADA_BENCH_FIXTURES", f"{SCR}/fixtures")
SPF = os.environ.get("RADA_BENCH_SPF", "spf")
SESS = "bench"

PROGS = {
    "rada": dict(cmd=f"{RADA}/target/release/rada", exe="rada", xdg=f"{SCR}/xdg", env=""),
    "spf": dict(cmd=SPF, exe="spf", xdg=f"{SCR}/xdg_cmp", env=f"_ZO_DATA_DIR={SCR}/xdg_cmp/zoxide"),
    "yazi": dict(cmd="yazi", exe="yazi", xdg=f"{SCR}/xdg_cmp", env=f"YAZI_CONFIG_HOME={SCR}/xdg_cmp/yazi_cfg"),
}

CASES = [
    ("14 files", f"{FIXTURES}/code", "binary.dat"),
    ("10,000 files", f"{FIXTURES}/many10k", "file_00000"),
    ("3,000 folders", f"{FIXTURES}/dirs3k", "dir_0000"),
]


def sh(*a):
    return subprocess.run(list(a), capture_output=True, text=True)


def start(name, cwd, w=160, h=50):
    p = PROGS[name]
    sh("tmux", "kill-session", "-t", SESS)
    x = p["xdg"]
    env = (f"XDG_CONFIG_HOME={x}/config XDG_DATA_HOME={x}/data XDG_STATE_HOME={x}/state "
           f"XDG_CACHE_HOME={x}/cache TERM=xterm-256color COLORTERM=truecolor {p['env']}")
    sh("tmux", "new-session", "-d", "-s", SESS, "-x", str(w), "-y", str(h), "-c", cwd,
       f"env {env} {p['cmd']}; sleep 60")


def snap():
    return sh("tmux", "capture-pane", "-t", SESS, "-p").stdout


def kill():
    sh("tmux", "kill-session", "-t", SESS)


def find_pid(name):
    exe = PROGS[name]["exe"]
    pane = sh("tmux", "list-panes", "-t", SESS, "-F", "#{pane_pid}").stdout.split()[0]
    todo, seen = [pane], set()
    while todo:
        cur = todo.pop()
        if cur in seen:
            continue
        seen.add(cur)
        try:
            if os.path.basename(os.readlink(f"/proc/{cur}/exe")) == exe:
                return int(cur)
        except OSError:
            pass
        todo += sh("pgrep", "-P", cur).stdout.split()
    return None


def rss(pid):
    d = {}
    for line in open(f"/proc/{pid}/status"):
        if line.startswith(("VmRSS", "VmHWM", "Threads")):
            k, v = line.split(":")
            d[k] = v.strip()
    return d


def cpu(pid):
    f = open(f"/proc/{pid}/stat").read().split(")")[1].split()
    return (int(f[11]) + int(f[12])) / os.sysconf("SC_CLK_TCK")


def startup(name, n=10):
    for label, d, needle in CASES:
        ts = []
        for _ in range(n):
            kill()
            t0 = time.perf_counter()
            start(name, d)
            while needle not in snap() and time.perf_counter() - t0 < 20:
                pass
            ts.append((time.perf_counter() - t0) * 1000)
            kill()
        ts.sort()
        print(f"{name:5s} startup {label:14s} min={ts[0]:.0f} median={statistics.median(ts):.0f} max={ts[-1]:.0f} ms (n={n})", flush=True)


COUNTER = {
    "rada": re.compile(r"(\d+)/10000 items"),
    "yazi": re.compile(r"(\d+)/10000"),
}


def counter(name, text):
    if name == "spf":
        for line in text.splitlines():
            if "Browser" in line:
                m = re.search(r"┤(\d+)/(\d+)├", line.split("Browser")[-1])
                if m:
                    return m.group(1)
        return None
    m = COUNTER[name].findall(text)
    return m[-1] if m else None


def latency(name, n=200):
    start(name, f"{FIXTURES}/many10k")
    t0 = time.perf_counter()
    while "file_00000" not in snap() and time.perf_counter() - t0 < 20:
        pass
    time.sleep(1.0)
    pid = find_pid(name)
    idle_rss = rss(pid)
    c0 = cpu(pid)
    time.sleep(10)
    idle_cpu = cpu(pid) - c0
    lat = []
    for i in range(n):
        before = counter(name, snap())
        t0 = time.perf_counter()
        sh("tmux", "send-keys", "-t", SESS, "j" if i % 2 == 0 else "k")
        while counter(name, snap()) == before and time.perf_counter() - t0 < 5:
            pass
        lat.append((time.perf_counter() - t0) * 1000)
        time.sleep(0.03)
    s = sorted(lat)
    print(f"{name:5s} latency 10k files n={n} p50={statistics.median(lat):.0f} p90={s[int(n*.9)]:.0f} "
          f"p99={s[int(n*.99)]:.0f} max={s[-1]:.0f} ms | idle RSS={idle_rss['VmRSS']} after keys {rss(pid)['VmRSS']} "
          f"HWM={rss(pid)['VmHWM']} threads={rss(pid)['Threads']} | idle CPU {idle_cpu:.2f}s/10s", flush=True)
    kill()


if __name__ == "__main__":
    what = sys.argv[1] if len(sys.argv) > 1 else "all"
    names = sys.argv[2:] or ["rada", "spf", "yazi"]
    for nm in names:
        if what in ("startup", "all"):
            startup(nm)
        if what in ("latency", "all"):
            latency(nm)
