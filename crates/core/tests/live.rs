//! Workers, watcher and jobs, driven the way the UI drives them: requests out, events in.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, unbounded};

use vela_core::events::*;
use vela_core::jobs::Jobs;
use vela_core::model::{DirListing, SortSpec};
use vela_core::ops::*;
use vela_core::platform::{
    Dirs, FileAttributes, Opener, PathRules, Platform, TrashBackend, Volume, VolumeLister,
};
use vela_core::testutil::*;
use vela_core::watch::DirWatcher;
use vela_core::workers::*;

/// Wait for an event matching `pick`; panic after `secs`.
fn wait_for<T>(
    rx: &Receiver<CoreEvent>,
    secs: u64,
    mut pick: impl FnMut(CoreEvent) -> Option<T>,
) -> T {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        let left = end.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(ev) => {
                if let Some(v) = pick(ev) {
                    return v;
                }
            }
            Err(_) => panic!("timed out after {secs}s waiting for an event"),
        }
    }
}

// ----------------------------------------------------------------------------- folders

#[test]
fn directory_loading_reports_the_real_error_for_unreadable_folders() {
    // superfile B9: "No such file or directory" instead of "Permission denied".
    if is_root() {
        return;
    }
    let sb = Sandbox::new();
    sb.mkdir("locked");
    chmod(&sb.path("locked"), 0o000);
    let (tx, rx) = unbounded();
    let loader = DirLoader::spawn(Arc::new(vela_core::fs::LocalFs), sb.platform(), tx);
    loader.load(sb.path("locked"), 1);
    let result = wait_for(&rx, 5, |ev| match ev {
        CoreEvent::Dir(DirEvent::Loaded { result, .. }) => Some(result),
        _ => None,
    });
    chmod(&sb.path("locked"), 0o755);
    let msg = result.unwrap_err();
    assert!(msg.to_lowercase().contains("permission denied"), "{msg}");
    assert!(msg.contains("locked"), "full path in the message: {msg}");
}

#[test]
fn only_the_newest_load_request_is_answered_under_pressure() {
    let sb = Sandbox::new();
    for i in 0..5 {
        sb.write(format!("d{i}/f.txt"), "x");
    }
    let (tx, rx) = unbounded();
    let loader = DirLoader::spawn(Arc::new(vela_core::fs::LocalFs), sb.platform(), tx);
    for i in 0..5 {
        loader.load(sb.path(format!("d{i}")), i);
    }
    let last_gen = wait_for(&rx, 5, |ev| match ev {
        CoreEvent::Dir(DirEvent::Loaded { generation, .. }) if generation == 4 => Some(generation),
        _ => None,
    });
    assert_eq!(last_gen, 4);
}

#[test]
fn a_huge_folder_loads_quickly_and_sorts_deterministically() {
    let sb = Sandbox::new();
    let dir = sb.mkdir("many");
    for i in 0..10_000 {
        std::fs::write(dir.join(format!("file{i:05}.txt")), "").unwrap();
    }
    let (tx, rx) = unbounded();
    let loader = DirLoader::spawn(Arc::new(vela_core::fs::LocalFs), sb.platform(), tx);
    let started = Instant::now();
    loader.load(dir.clone(), 1);
    let entries = wait_for(&rx, 10, |ev| match ev {
        CoreEvent::Dir(DirEvent::Loaded { result, .. }) => Some(result.unwrap()),
        _ => None,
    });
    assert_eq!(entries.len(), 10_000);
    eprintln!("loaded 10k entries in {:?}", started.elapsed());
    let t = Instant::now();
    let listing = DirListing::new(dir, entries, SortSpec::default());
    assert!(
        t.elapsed() < Duration::from_millis(250),
        "sorting 10k took {:?}",
        t.elapsed()
    );
    assert_eq!(listing.all()[0].display, "file00000.txt");
    assert_eq!(listing.all()[9_999].display, "file09999.txt");
}

// ----------------------------------------------------------------------------- watcher

fn changed(rx: &Receiver<CoreEvent>, secs: u64) -> (PathBuf, Vec<PathBuf>, bool) {
    wait_for(rx, secs, |ev| match ev {
        CoreEvent::Watch(WatchEvent::Changed { dir, paths, rescan }) => Some((dir, paths, rescan)),
        _ => None,
    })
}

#[test]
fn files_created_removed_and_renamed_from_outside_show_up_without_any_key() {
    // superfile B6/#928: an externally created file was invisible for >12 s.
    let sb = Sandbox::new();
    let dir = sb.mkdir("watched");
    let (tx, rx) = unbounded();
    let w = DirWatcher::spawn(tx);
    w.watch(dir.clone());
    std::thread::sleep(Duration::from_millis(200)); // let the watch register

    let started = Instant::now();
    std::fs::write(dir.join("ext_created.txt"), "hi").unwrap();
    let (d, paths, rescan) = changed(&rx, 3);
    assert_eq!(d, dir);
    assert!(
        rescan || paths.contains(&dir.join("ext_created.txt")),
        "{paths:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "took {:?}",
        started.elapsed()
    );

    std::fs::rename(dir.join("ext_created.txt"), dir.join("renamed.txt")).unwrap();
    let (_, paths, rescan) = changed(&rx, 3);
    assert!(
        rescan
            || (paths.contains(&dir.join("renamed.txt"))
                && paths.contains(&dir.join("ext_created.txt"))),
        "{paths:?}"
    );

    std::fs::remove_file(dir.join("renamed.txt")).unwrap();
    let (_, paths, rescan) = changed(&rx, 3);
    assert!(rescan || paths.contains(&dir.join("renamed.txt")));
}

#[test]
fn a_burst_of_changes_is_coalesced_into_few_events() {
    let sb = Sandbox::new();
    let dir = sb.mkdir("burst");
    let (tx, rx) = unbounded();
    let w = DirWatcher::spawn(tx);
    w.watch(dir.clone());
    std::thread::sleep(Duration::from_millis(200));
    for i in 0..2000 {
        std::fs::write(dir.join(format!("f{i}")), "").unwrap();
    }
    let (_, paths, rescan) = changed(&rx, 3);
    assert!(
        rescan && paths.is_empty(),
        "a big burst asks for one rescan"
    );
    std::thread::sleep(Duration::from_millis(800));
    let extra = rx
        .try_iter()
        .filter(|e| matches!(e, CoreEvent::Watch(WatchEvent::Changed { .. })))
        .count();
    assert!(extra <= 6, "{extra} extra events for 2000 files");
}

#[test]
fn live_patches_update_a_sorted_listing() {
    let sb = Sandbox::new();
    let dir = sb.mkdir("live");
    sb.write("live/b.txt", "b");
    let (tx, rx) = unbounded();
    let fs: Arc<dyn vela_core::fs::FsEngine> = Arc::new(vela_core::fs::LocalFs);
    let loader = DirLoader::spawn(fs, sb.platform(), tx.clone());
    let watcher = DirWatcher::spawn(tx);
    let mut listing = DirListing::new(dir.clone(), vec![], SortSpec::default());
    loader.load(dir.clone(), 1);
    watcher.watch(dir.clone());
    std::thread::sleep(Duration::from_millis(250));
    sb.write("live/a.txt", "a");
    sb.write("live/c.txt", "c");

    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        let Ok(ev) = rx.recv_timeout(Duration::from_millis(200)) else {
            continue;
        };
        match ev {
            CoreEvent::Dir(DirEvent::Loaded { result, .. }) => {
                listing = DirListing::new(dir.clone(), result.unwrap(), SortSpec::default())
            }
            CoreEvent::Dir(DirEvent::Patched { updates, .. }) => listing.apply(updates),
            CoreEvent::Watch(WatchEvent::Changed {
                dir: d,
                paths,
                rescan,
            }) => {
                if rescan {
                    loader.load(d, 2)
                } else {
                    loader.patch(d, paths, 2)
                }
            }
            _ => {}
        }
        if listing.len() == 3 {
            break;
        }
    }
    let names: Vec<_> = listing.all().iter().map(|e| e.display.clone()).collect();
    assert_eq!(names, ["a.txt", "b.txt", "c.txt"]);
}

// ----------------------------------------------------------------------------- volumes

struct SlowVolumes;
impl VolumeLister for SlowVolumes {
    fn list(&self) -> vela_core::Result<Vec<Volume>> {
        std::thread::sleep(Duration::from_secs(2)); // a hung network mount
        Ok(vec![])
    }
}

struct WithSlowVolumes(Arc<dyn Platform>, SlowVolumes);
impl Platform for WithSlowVolumes {
    fn name(&self) -> &'static str {
        self.0.name()
    }
    fn dirs(&self) -> &Dirs {
        self.0.dirs()
    }
    fn trash(&self) -> &dyn TrashBackend {
        self.0.trash()
    }
    fn volumes(&self) -> &dyn VolumeLister {
        &self.1
    }
    fn attributes(&self) -> &dyn FileAttributes {
        self.0.attributes()
    }
    fn opener(&self) -> &dyn Opener {
        self.0.opener()
    }
    fn path_rules(&self) -> PathRules {
        self.0.path_rules()
    }
}

#[test]
fn a_slow_volume_listing_never_blocks_the_caller() {
    // superfile #1669 / #727: a slow mount froze every keypress.
    let sb = Sandbox::new();
    let p: Arc<dyn Platform> = Arc::new(WithSlowVolumes(sb.platform(), SlowVolumes));
    let (tx, rx) = unbounded();
    let t = Instant::now();
    let w = VolumesWorker::spawn(p, Duration::from_secs(5), tx);
    w.refresh_now();
    assert!(
        t.elapsed() < Duration::from_millis(100),
        "spawn returned in {:?}",
        t.elapsed()
    );
    assert!(
        rx.try_recv().is_err(),
        "nothing yet: the listing is still running elsewhere"
    );
    let vols = wait_for(&rx, 6, |ev| match ev {
        CoreEvent::Volumes(v) => Some(v),
        _ => None,
    });
    assert!(vols.is_empty());
}

#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "volume enumeration is implemented for Linux only; Windows and macOS are in development"
)]
fn the_real_volume_list_arrives_by_event() {
    let sb = Sandbox::new();
    let (tx, rx) = unbounded();
    let _w = VolumesWorker::spawn(sb.platform(), Duration::from_secs(5), tx);
    let vols = wait_for(&rx, 10, |ev| match ev {
        CoreEvent::Volumes(v) => Some(v),
        _ => None,
    });
    assert!(
        vols.iter()
            .any(|v| v.mount_point == std::path::Path::new("/"))
    );
}

// ----------------------------------------------------------------------------- jobs

fn jobs_for(sb: &Sandbox) -> (Jobs, Receiver<CoreEvent>) {
    let (tx, rx) = unbounded();
    let journal = Arc::new(sb.journal());
    (Jobs::new(sb.engine(), Some(journal), tx), rx)
}

fn job_event<T>(rx: &Receiver<CoreEvent>, mut pick: impl FnMut(JobEvent) -> Option<T>) -> T {
    wait_for(rx, 20, |ev| match ev {
        CoreEvent::Job(j) => pick(j),
        _ => None,
    })
}

#[test]
fn plan_then_confirm_then_run_then_undo_through_the_job_api() {
    let sb = Sandbox::new();
    sb.write("src/a.txt", "a");
    sb.write("src/sub.v1/b.bin", vec![1u8; 5_000_000]);
    let dest = sb.mkdir("dest");
    let (jobs, rx) = jobs_for(&sb);

    // 1. plan
    let h = jobs.plan_transfer(
        vec![sb.path("src")],
        dest.clone(),
        TransferOptions::copy(ConflictPolicy::Skip),
    );
    let (plan, scan) = job_event(&rx, |j| match j {
        JobEvent::Planned {
            job, plan, scan, ..
        } if job == h.id => Some((plan, scan)),
        _ => None,
    });
    assert_eq!(plan.totals.files, 2);
    assert!(plan.is_executable());
    assert!(!dest.join("src").exists(), "planning changed nothing");

    // 1b. changing the policy re-plans from the cached scan
    let h2 = jobs.replan_transfer(
        scan.unwrap(),
        dest.clone(),
        TransferOptions::copy(ConflictPolicy::KeepBoth),
    );
    let plan2 = job_event(&rx, |j| match j {
        JobEvent::Planned { job, plan, .. } if job == h2.id => Some(plan),
        _ => None,
    });
    assert_eq!(plan2.policy, ConflictPolicy::KeepBoth);

    // 2. confirm -> execute, with byte progress
    let h3 = jobs.execute(*plan);
    let mut saw_progress = false;
    let (report, id) = job_event(&rx, |j| match j {
        JobEvent::Progress { job, progress } if job == h3.id => {
            saw_progress |= progress.bytes_total == 5_000_001;
            None
        }
        JobEvent::Finished {
            job,
            report,
            journal_id,
            ..
        } if job == h3.id => Some((report, journal_id)),
        _ => None,
    });
    assert!(saw_progress);
    assert_eq!(report.status(), RunStatus::Completed);
    assert!(id.is_some());
    assert!(dest.join("src/sub.v1/b.bin").exists());

    // 3. history
    let hh = jobs.load_history();
    let entries = job_event(&rx, |j| match j {
        JobEvent::History { job, entries } if job == hh.id => Some(entries),
        _ => None,
    });
    assert_eq!(entries.len(), 1);
    assert!(entries[0].is_undoable());

    // 4. undo the last operation, with its own plan
    let hu = jobs.plan_undo(None);
    let up = job_event(&rx, |j| match j {
        JobEvent::Planned { job, undo, .. } if job == hu.id => undo,
        _ => None,
    });
    assert!(up.blocked.is_empty());
    let hx = jobs.execute_undo(*up);
    job_event(&rx, |j| match j {
        JobEvent::Finished { job, .. } if job == hx.id => Some(()),
        _ => None,
    });
    assert!(!dest.join("src").exists());

    // 5. nothing left to undo
    let hn = jobs.plan_undo(None);
    job_event(&rx, |j| match j {
        JobEvent::NothingToUndo { job } if job == hn.id => Some(()),
        _ => None,
    });
}

#[test]
fn a_failure_blocks_only_the_worker_until_the_user_answers() {
    if is_root() {
        return;
    }
    let sb = Sandbox::new();
    sb.write("src/a.txt", "a");
    let bad = sb.write("src/b.txt", "b");
    sb.write("src/c.txt", "c");
    chmod(&bad, 0o000);
    let dest = sb.mkdir("dest");
    let (jobs, rx) = jobs_for(&sb);
    let h = jobs.plan_transfer(
        vec![sb.path("src")],
        dest.clone(),
        TransferOptions::copy(ConflictPolicy::Skip),
    );
    let plan = job_event(&rx, |j| match j {
        JobEvent::Planned { job, plan, .. } if job == h.id => Some(plan),
        _ => None,
    });
    let h = jobs.execute(*plan);
    let mut asked = 0;
    let report = job_event(&rx, |j| match j {
        JobEvent::AskFailure { info, reply, .. } => {
            asked += 1;
            assert!(info.message.contains("b.txt"), "{}", info.message);
            assert!(info.message.to_lowercase().contains("permission denied"));
            reply.send(ErrorChoice::Skip).unwrap();
            None
        }
        JobEvent::Finished { job, report, .. } if job == h.id => Some(report),
        _ => None,
    });
    chmod(&bad, 0o644);
    assert_eq!(asked, 1);
    assert_eq!(report.failed.len(), 1);
    assert!(dest.join("src/a.txt").exists() && dest.join("src/c.txt").exists());
}

#[test]
fn cancelling_a_job_stops_it() {
    let sb = Sandbox::new();
    sb.write("big/a.bin", vec![1u8; 80 << 20]);
    sb.write("big/b.bin", vec![2u8; 80 << 20]);
    let dest = sb.mkdir("dest");
    let (jobs, rx) = jobs_for(&sb);
    let h = jobs.plan_transfer(
        vec![sb.path("big")],
        dest.clone(),
        TransferOptions::copy(ConflictPolicy::Skip),
    );
    let plan = job_event(&rx, |j| match j {
        JobEvent::Planned { job, plan, .. } if job == h.id => Some(plan),
        _ => None,
    });
    let h = jobs.execute(*plan);
    let report = job_event(&rx, |j| match j {
        JobEvent::Progress { job, progress } if job == h.id && progress.bytes_done > (4 << 20) => {
            h.cancel.cancel();
            None
        }
        JobEvent::Finished { job, report, .. } if job == h.id => Some(report),
        _ => None,
    });
    assert_eq!(report.status(), RunStatus::Cancelled);
}
