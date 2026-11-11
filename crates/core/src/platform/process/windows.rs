//! The Windows part: the program runs inside a Job Object that is closed with it.
//!
//! The process is created suspended, put in the job and only then let run, so nothing it
//! starts can be outside the job. Closing the job's handle kills whatever is still in it, and
//! Windows closes the handle by itself if rada dies.

use std::io;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{
    BELOW_NORMAL_PRIORITY_CLASS, CREATE_NO_WINDOW, CREATE_SUSPENDED, OpenThread, ResumeThread,
    THREAD_SUSPEND_RESUME,
};

/// The job the program runs in.
pub(super) struct Guard {
    job: HANDLE,
}

impl Drop for Guard {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by `new_job` and is closed only here.
        unsafe {
            CloseHandle(self.job);
        }
    }
}

/// Create the process stopped, without a console window and at a low priority.
pub(super) fn prepare(cmd: &mut Command) {
    cmd.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
}

/// Put the stopped process in a job of its own and let it run.
pub(super) fn adopt(child: &Child) -> io::Result<Guard> {
    let guard = Guard { job: new_job()? };
    // SAFETY: both handles are open: the job is owned by `guard`, the process by `child`.
    if unsafe { AssignProcessToJobObject(guard.job, child.as_raw_handle() as HANDLE) } == 0 {
        return Err(io::Error::last_os_error());
    }
    resume(child.id())?;
    Ok(guard)
}

/// A job that kills what is in it when its handle is closed.
fn new_job() -> io::Result<HANDLE> {
    // SAFETY: both arguments may be null: default security, no name.
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the structure is plain numbers, for which all zeroes means "no limit".
    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let size = std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
    // SAFETY: `info` is of the type and size this class of information asks for, and lives
    // across the call.
    let set = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&raw const info).cast(),
            size,
        )
    };
    if set == 0 {
        let e = io::Error::last_os_error();
        // SAFETY: the handle was just opened here and is not used after.
        unsafe {
            CloseHandle(job);
        }
        return Err(e);
    }
    Ok(job)
}

/// Let the threads of process `pid` run. The standard library keeps the handle of the first
/// thread to itself, so the threads are found in a list of the system's.
fn resume(pid: u32) -> io::Result<()> {
    // SAFETY: no pointer is passed; the result is checked before use.
    let list = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if list == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the structure is plain numbers; its size field is set as the call requires.
    let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
    let mut resumed = 0;
    // SAFETY: `list` is an open list of threads and `entry` a valid place to write.
    let mut more = unsafe { Thread32First(list, &mut entry) } != 0;
    while more {
        if entry.th32OwnerProcessID == pid && resume_thread(entry.th32ThreadID) {
            resumed += 1;
        }
        // SAFETY: as for Thread32First.
        more = unsafe { Thread32Next(list, &mut entry) } != 0;
    }
    // SAFETY: the list was opened here and is not used after.
    unsafe {
        CloseHandle(list);
    }
    if resumed == 0 {
        return Err(io::Error::other("the program could not be let run"));
    }
    Ok(())
}

/// Let one thread run; whether it was done.
fn resume_thread(id: u32) -> bool {
    // SAFETY: no pointer is passed; a thread that is gone gives a null handle.
    let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, id) };
    if thread.is_null() {
        return false;
    }
    // SAFETY: the handle is open, used for this one call and closed right after.
    unsafe {
        let resumed = ResumeThread(thread) != u32::MAX;
        CloseHandle(thread);
        resumed
    }
}

impl Guard {
    /// Whether the program has ended.
    pub(super) fn exited(&self, child: &mut Child) -> bool {
        !matches!(child.try_wait(), Ok(None))
    }

    /// End everything in the job. Windows has no way to ask first.
    pub(super) fn stop(&self, _child: &mut Child) {
        self.sweep();
    }

    /// End what is still in the job.
    pub(super) fn sweep(&self) {
        // SAFETY: the job's handle is open for as long as `self` lives.
        unsafe {
            TerminateJobObject(self.job, 1);
        }
    }
}
