# Architecture

A map of the code for someone opening the repository for the first time. For build and test
commands see [development.md](development.md); for the safety rules see [SAFETY.md](SAFETY.md).

## The three crates

| Crate | What it does |
|---|---|
| `crates/core` (`rada-core`) | Everything that is not a screen. The file model, the plan → execute → journal → undo engine, the background workers, the platform layer. Depends on no interface. |
| `crates/tui` (`rada-tui`) | The terminal interface (ratatui + crossterm). `App` holds the state, `ui/` draws it, `run.rs` is the event loop. Never changes a file itself: it asks `rada-core`. |
| `crates/rada` (`rada`) | The binary. Parses the command line and `config.toml`, detects the terminal, builds `Config` and `Services`, calls `rada_tui::run::run`. Also the installers: desktop entry, shell hook, fonts, `Rada.app`, Windows folder setup. |

## Main flow

Every change to a file goes through the same five steps. No operation skips the plan or the journal.

```mermaid
flowchart LR
    K[key / mouse] --> A["App<br/>on_key, on_mouse"]
    A -->|OpRequest| J["Jobs<br/>jobs.rs"]
    J --> P["plan<br/>request.rs + planner.rs<br/>→ Plan of Steps"]
    P -->|JobEvent::Planned| M["App shows the plan<br/>Modal::Plan"]
    M -->|confirm| X["exec<br/>Engine::execute"]
    X --> R["journal<br/>begin, pending, undo, end"]
    R -->|U / u| UP["undo.rs<br/>journal entry → Plan"]
    UP --> X
```

1. **Input.** `app/input.rs` turns a key into a `keymap::Action` and calls the matching `App` method; `app/mouse.rs` does the same for clicks and the wheel.
2. **Plan.** `Jobs::plan_request` scans the sources (`ops/scan.rs`) and builds a `Plan`: a list of `Step`s (`MakeDir`, `CopyFile`, `Rename`, `TrashItem`, …). Nothing on disk changes. The `App` shows the plan and waits.
3. **Execute.** After the confirmation, `Jobs::execute` runs the plan on a worker thread (`ops/exec.rs`). Errors ask the person what to do (skip, retry, abort) through `JobEvent::AskFailure`.
4. **Journal.** `ops/runner.rs` wraps the executor. Before a step it writes `pending`; after it, the inverse step. The journal is an append-only JSON-lines file in the state folder. If the journal cannot be written when an operation begins, the operation does not start. If the file could not be opened at launch, rada warns and runs without undo.
5. **Undo.** `ops/undo.rs` reads a journal entry, checks that the disk is still as the entry left it, and builds a new `Plan` made of the inverse steps. That plan runs through the same executor and is journaled too. Redo re-plans the original request.

A program can enter at step 2: `rada --request file.json` builds an `OpRequest` and the confirmation window opens on it. `rada --schema request` prints the JSON Schema.

## Background work

The UI thread does no I/O. Every slow or blocking job runs on a worker thread of `rada-core` and answers on **one** channel of `CoreEvent` (`core/src/events.rs`).

- `Services::start_with` (`tui/src/services.rs`) starts the workers and keeps their senders: `Jobs`, `DirLoader` (two: current folder and its parent), `PreviewWorker`, `SizeWorker`, `DirWatcher`, `VolumesWorker`, `PlacesStore`, plus one-shot threads for git, zoxide, tree and completion lists.
- `run.rs` `select!`s over three inputs: the keyboard (its own thread), the `CoreEvent` channel and image-resize results. A burst of events folds into one frame.
- `App::on_core_event` is the only place where an answer from a worker becomes state. It routes by variant: `Dir`, `Watch`, `Preview`, `Job`, `Volumes`, `Git`, …
- Requests carry a **generation** number. The `App` bumps it on every navigation and drops answers with an older one, so a slow listing of a folder already left cannot overwrite the current one.
- `ui::draw(f, &mut App)` reads only `App` state. It takes `&mut` to store what it learned about the screen (click rectangles in `Hits`, row counts in `Viewport`), never to touch the disk. All file data it shows (`DirListing`, `Preview`, sizes, git status) arrived earlier as an event.
- Known exception: the save-as check of the file chooser stats one path on the UI thread (`app/chooser.rs`). The terminal probes in `termtheme.rs` and `clipboard.rs` run programs, not files.

## The state of `App`

`App` (`crates/tui/src/app.rs`) has 19 fields. Each is a group that owns one kind of state, with its methods next to it; `app.rs` keeps the event loop entry points and what crosses groups.

| Field | Holds | Where |
|---|---|---|
| `settings` | What `config.toml` and the View menu set: theme, icons, keymap, date formats, layout. Read all the time, changes seldom. | `app/settings.rs` |
| `svc` | The workers and their senders (`Services`), seen only from `App`. | `services.rs` |
| `hooks` | What only tests and screenshots use: the demo scene, made-up disks, a settable clock. | `app.rs` (`TestHooks`) |
| `tab` | The front tab: folder, listing, filter, sort, view, history, and its `Selection` (cursor, scroll, marks). | `app/tabs.rs`, `app/listing.rs`, `app/selection.rs` |
| `tabs` | The other tabs, which one is in front, and what is saved between sessions. | `app/tabs.rs` |
| `preview` | The preview of the item under the cursor, its picture, and the keyboard focus in it. | `app/preview.rs` |
| `devices` | Disks, sticks and phones, and the folders the system holds back. | `app/mount.rs` |
| `places`, `tags` | The places the sidebar lists, and the tags the sidebar and the tag menu know. | `app/side.rs` |
| `ops` | The clipboard, the plan being prepared, the operation running, the undo label. | `app/ops.rs`, `app/jobs.rs`, `app/outcome.rs`, `app/prompt.rs` |
| `windows` | The open window (`Modal`), the notification cards, the address bar, the help scroll. | `app/windows.rs`, `app/bar.rs` |
| `columns` | The Columns view: the parent column and its read. | `app/columns.rs` |
| `session` | How the session ends, and whether it is a file chooser. | `app/chooser.rs` |
| `redraw` | Whether a frame is due, a full redraw, the spinner. | `app.rs` (`Redraw`) |
| `side_nav` | The sidebar and the NAV box: the keyboard in them, their cursors, the tree. | `app/side.rs`, `app/navrows.rs` |
| `frame` | What the last frame drew (click rectangles, areas), for the mouse and the tests. | `ui/frame.rs` (`LastFrame`) |
| `folder_info` | What is worked out about the folder shown: git status, folder sizes, date cache. | `app/panels.rs`, `app/sizes.rs` |
| `updates` | The version, what the update worker last told, whether somebody waits for an answer. | `app/update.rs` |
| `last_click` | The time and row of the last click, for the double click. | `app.rs` |

A method sits next to the data it uses. One that reads only a group is a method of that group (`TabState::current`, `Redraw::after_launch`). One that reaches several groups, or the workers in `svc`, stays a method of `App` and sits in the file of its subject (`toast` in `app/windows.rs`). `app.rs` keeps what only the whole `App` can do: building it, the event loop's entry points and the sending of events to the groups.

The rest of `app/` is by subject: `input.rs` (keys and `dispatch`), `mouse.rs`, `actions.rs` (the explorer's own commands), `menus.rs` and `menuview.rs`, `address.rs` and `bar.rs` (the address and search bar), `palette.rs`, `navigate.rs` (going to another folder), `load.rs` (reading one), `listing.rs`, `prompt.rs` (the questions asked with a line of text), `jobs.rs` (what the worker says about an operation), `outcome.rs` (the card that says it went through), `demo.rs` (the scenes of `--demo`). No function there is longer than 40 lines.

### Tabs

Everything that works on "the folder" reads `App::tab`, a `TabState`: the front tab. The tabs behind it sit in `tabs.slots`, one `TabState` each, and the slot of the front tab is an empty `TabState::default()` so that the indices of `slots` are the indices of the tabs; `tabs.active` says which one is in front.

To change the front tab, `App::bring_to_front(incoming)` does `mem::replace(&mut self.tab, incoming)` and hands back the tab that left, to be put in its slot. `switch_tab`, `new_tab` and `close_tab` all go through it. `TabState::leave` runs on the tab that goes out and drops what only makes sense in front: a read of the folder still on its way (`load`) and a pending history move (`nav_move`). No field is copied one by one, so a new per-tab field only has to be added to `TabState`.

## Platform code

All OS-dependent behaviour sits behind the `Platform` trait (`core/src/platform/mod.rs`): trash, volumes, file attributes, opening files, path rules, tags, user folders. The rest of the core never tests the OS; it asks the trait.

| Platform | Where |
|---|---|
| Linux | `core/src/platform/linux/` (`devices.rs` phones and sticks, `mounts.rs`, `open.rs`, `fidelity.rs`), `freedesktop.rs` (trash), `unix.rs` |
| Windows | `core/src/platform/windows.rs` (Recycle Bin, drives, reparse points, attributes). Binary side: `rada/src/winsetup.rs`, `winfolder.rs` |
| macOS | `core/src/platform/macos.rs` (Finder trash, volumes, tags, Quick Look), `bplist.rs`, `foldernames.rs`. Binary side: `rada/src/macapp.rs` |
| Shared | `openwith.rs`, `tags.rs`, `launch.rs`, `access.rs` |

`windows.rs` and `macos.rs` are compiled on every OS (the Windows and macOS code is gated with `cfg` inside the file), so Linux can type-check them. `platform::current` picks the implementation; `testplatform.rs` is the fake used by tests.

## Read these first, in this order

1. `crates/core/src/ops/plan.rs` — `Step`, `Plan`, `Step::inverse`. The vocabulary of the engine.
2. `crates/core/src/ops/request.rs` — `OpRequest` and `plan_request`: from a request to a `Plan`.
3. `crates/core/src/ops/exec.rs` — `Engine::execute`, `run_step`, error handling, cancellation.
4. `crates/core/src/journal.rs` — the on-disk record and how it is replayed.
5. `crates/core/src/ops/undo.rs` — journal entry → inverse `Plan`, with the safety checks.
6. `crates/core/src/events.rs` — every message a worker can send. The contract between core and UI.
7. `crates/core/src/jobs.rs` — how plan, execute and undo run on threads and report back.
8. `crates/tui/src/run.rs` — the event loop: terminal setup, `select!`, drawing.
9. `crates/tui/src/app.rs` — the `App` struct, `on_core_event`, `on_job`, `on_modal_key`.
10. `crates/tui/src/ui/mod.rs` — `draw`: how the screen is composed from `App` state.
