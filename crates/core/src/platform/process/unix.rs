//! The Unix part: the program leads a process group of its own, and signals go to the group.

use std::io;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::time::Instant;

use super::{GRACE, POLL};

/// How much lower than rada's the program's priority is.
const NICE: libc::c_int = 10;

/// The group the program leads.
pub(super) struct Guard {
    pgid: libc::pid_t,
}

/// Make the program lead a new group, run at a low priority and, on Linux, die with rada.
pub(super) fn prepare(cmd: &mut Command) {
    cmd.process_group(0);
    let parent = std::process::id() as libc::pid_t;
    // SAFETY: the closure runs in the child between fork and exec, where only
    // async-signal-safe calls are allowed: it makes plain system calls (setpriority, prctl,
    // getppid), takes no lock and allocates nothing unless one of them fails.
    unsafe {
        cmd.pre_exec(move || before_exec(parent));
    }
}

/// What the child does before becoming the program.
fn before_exec(parent: libc::pid_t) -> io::Result<()> {
    // SAFETY: a system call on this very process, with constant arguments. A refusal is not
    // worth failing for: the program runs at the usual priority.
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS as _, 0, NICE);
    }
    die_with(parent)
}

/// Ask the kernel to kill this process when the thread that started it ends, so a rada that
/// crashes does not leave the program running. The parent may have gone already, before the
/// request was made: then there is no one to run for.
#[cfg(target_os = "linux")]
fn die_with(parent: libc::pid_t) -> io::Result<()> {
    // SAFETY: system calls on this very process, with constant arguments and no pointer.
    unsafe {
        if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::getppid() != parent {
            return Err(io::Error::from_raw_os_error(libc::ESRCH));
        }
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn die_with(_parent: libc::pid_t) -> io::Result<()> {
    Ok(())
}

/// The group of a program that was just started.
pub(super) fn adopt(child: &Child) -> io::Result<Guard> {
    Ok(Guard {
        pgid: child.id() as libc::pid_t,
    })
}

impl Guard {
    /// Whether the program has ended. It is not reaped: until `Child::wait` does that, its
    /// number cannot be given to another process, so a signal to the group cannot reach a
    /// stranger.
    pub(super) fn exited(&self, _child: &mut Child) -> bool {
        // SAFETY: an all-zero siginfo_t is a valid value, and waitid only writes into it.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let flags = libc::WEXITED | libc::WNOHANG | libc::WNOWAIT;
        // SAFETY: `info` is a valid place to write and lives across the call.
        let r = unsafe { libc::waitid(libc::P_PID, self.pgid as libc::id_t, &mut info, flags) };
        // SAFETY: waitid filled the structure, or left it zeroed when nothing has ended.
        r != 0 || unsafe { info.si_pid() } != 0
    }

    /// Ask the group to end, give it a moment, then kill what is left of it.
    pub(super) fn stop(&self, child: &mut Child) {
        self.signal(libc::SIGTERM);
        let until = Instant::now() + GRACE;
        while !self.exited(child) && Instant::now() < until {
            std::thread::sleep(POLL);
        }
        self.signal(libc::SIGKILL);
    }

    /// Kill what the program left running in its group.
    pub(super) fn sweep(&self) {
        self.signal(libc::SIGKILL);
    }

    fn signal(&self, signal: libc::c_int) {
        // SAFETY: a system call with two numbers. The group is the program's own, never
        // rada's: it was made when the program started and its leader is not reaped yet.
        unsafe {
            libc::killpg(self.pgid, signal);
        }
    }
}
