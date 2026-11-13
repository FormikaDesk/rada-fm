# Recipes and actions

A **recipe** is a small Lua program that looks at the files you have selected and asks rada
for a plan: make a PDF out of these two, a smaller copy of these photos, a checksum. rada comes
with nine of them, the **actions**; you can write your own. Whatever a recipe asks goes through
the same plan window, the same journal and the same `Ctrl+Z` as everything else: a recipe cannot
change a file by itself.

## Using them

- **Right click** on what is selected (or `m`, `Shift+F10`, the Menu key): **Actions ›** lists
  the recipes for those files. A recipe that is not for them is not there; one that is but cannot
  run now is greyed, and says why on the right (*choose at least 2 items*).
- **The bar** (`Ctrl+P`): type part of the title (*merge*, *unisci*) and it is among the
  suggestions, as *action*.
- **The command line**: `:recipe merge-pdf`, or just `:merge-pdf`; settings go after it as
  `name=value` (`:rotate-pages angle=180 pages=1-3`). What is not given is asked.

A recipe's settings are asked one at a time with the windows rada already has: a menu for a
choice (the default is ticked and under the cursor), the input window for a text or a number.
`Esc` gives up. Then *Preparing…* while the recipe runs (`Esc` stops it), and the plan window.
While a program runs, the progress card shows it; `Esc` (or *Cancel* on the card) stops the
program and everything it started, and nothing is published.

### The actions that come with rada

Each is offered only when its program is installed. Chosen for several files, an action for one
file makes one new file for each, in one plan with one undo. The new file goes next to the
original, and never takes the place of anything: when the name is taken it becomes
`name (1).ext`.

| action | for | makes | runs |
|---|---|---|---|
| Merge PDFs | 2 or more PDFs | `first-merged.pdf`, the pages in the order of the list | `qpdf --empty --pages … -- out` |
| Extract pages | PDFs | `name-pages.pdf` with the pages asked (`1-3,7`; `z` is the last page, `r2` the one before) | `qpdf in --pages . 1-3,7 -- out` |
| Rotate pages | PDFs | `name-rotated.pdf`, turned 90, 180 or 270 degrees clockwise, all pages (`1-z`) or some | `qpdf in --rotate=+90:1-z out` |
| Compress PDF | PDFs | `name-ebook.pdf` (screen, ebook or printer) | Ghostscript, `-dPDFSETTINGS=/ebook -dSAFER` |
| Convert image | images | `name.png`, `.jpg` or `.webp` (`name-converted.png` for a PNG made into a PNG) | ImageMagick (`magick`; `convert` on Linux only); on a Mac without it, `sips` (not WebP) |
| Resize image | images | `name-1600.jpg`, the longer side at most 1600, 1080 or 800 pixels (a smaller image is left as it is) | `magick in -resize 1600x1600> out`; on a Mac, `sips -Z 1600` |
| Extract audio | videos | `name.m4a` (AAC, 192 kb/s) | `ffmpeg -nostdin -n -i in -vn -c:a aac -b:a 192k out` |
| Document to PDF | Word, Excel, PowerPoint, OpenDocument, RTF | `name.pdf` | LibreOffice, headless, with a profile of its own |
| SHA-256 checksum | any file | `name.ext.sha256`, as `sha256sum` writes it | nothing: rada computes it |

A page range that is not one (`--version`, `all`) is refused before qpdf sees it. A program that
is installed where rada does not look is named in `[recipes] programs` of `config.toml`.

## Writing one

Put a `.lua` file in the `recipes` folder next to `config.toml` (`~/.config/rada/recipes/` on
Linux, `~/Library/Application Support/rada/recipes/` on a Mac, `%APPDATA%\rada\recipes\` on
Windows). rada reads them when it starts; a file with a mistake is a notification with its file
and line (*Recipe not loaded: tidy.lua:12: attempt to call a nil value*), and the others are
there. The actions above are written the same way: their files are in `crates/lua/recipes/` of
the sources.

```lua
-- Copy the PDFs chosen into a folder "sent" beside them, made if it is not there.
rada.recipe {
  name = "send-pdfs",                       -- small letters, digits, - and _
  title = { en = "Put in sent", it = "Metti in inviati" },
  description = "A copy of each PDF in the folder sent",
  when = { ext = "pdf", min = 1 },          -- for which files it is offered
  args = {
    { name = "verify", type = "flag", label = "Compare the copies", default = false },
  },
  available = function(ctx)                 -- optional: can it run now?
    return true
  end,
  run = function(ctx, args)
    local sent = rada.join(ctx.cwd, "sent")
    if rada.stat(sent) == nil then
      rada.mkdir(sent)
    end
    rada.copy(ctx.selection, sent, { verify = args.verify })
    return { notes = { "The originals stay where they are." } }
  end,
}
```

A file can declare several recipes. A name already taken (by an action of rada's or by another
file) is refused, and the first one stays.

### `rada.recipe { … }`

| field | |
|---|---|
| `name` | what `:recipe` calls it; unique |
| `title`, `description` | a text, or one per language: `{ en = "…", it = "…" }` |
| `when.ext` | an extension or a list of them, any case (`"pdf"`, `{ "jpg", "png" }`); none: any |
| `when.kind` | what each item must be: `"file"` (the default), `"folder"` or `"any"` |
| `when.min`, `when.max` | how many items (`min` is 1 by default). Outside them the recipe is greyed with the reason, not hidden |
| `when.dir` | `true`: it is for the folder shown, and is offered with nothing selected |
| `args` | the settings asked before `run`: `{ name, label, type, default }` with `type` `"text"`, `"choice"` (with `choices = { … }`), `"int"` (with `min`, `max`) or `"flag"` |
| `available(ctx)` | optional. `true`: it can run. `false`: it is not offered. `false, "why"`: greyed, with why |
| `run(ctx, args)` | asks for the plan. It may return `{ title = "…", notes = { "…" } }` for the plan window |

`ctx` has `selection` (the marked items, or the one under the cursor, as whole paths),
`marked`, `cwd` (the folder shown), `platform` (`"linux"`, `"macos"`, `"windows"`) and `lang`
(`"en"`, `"it"`). `args` has a value for every setting.

### What `run` can call

**Look** (they change nothing):

| | |
|---|---|
| `rada.stat(path)` | `{ path, name, kind, size, mtime }` (`kind`: file, folder, link, other; a link is not followed), or `nil, why` |
| `rada.list(folder)` | its entries, sorted by name, as `rada.stat` gives them; at most 10 000, and a second value `true` when there were more |
| `rada.meta(path)` | what the file says about itself, as the details pane shows it: `exif = { date, time, camera, orientation, gps = { lat, lon }, place }`, `audio = { artist, title, album, track, year }`, `pdf = { title, author, pages }`, `video = { duration, width, height }`; what is not there is `nil` |
| `rada.has(program)` | whether a program an action may run is installed (`"qpdf"`, `"gs"`, `"magick"`, `"sips"`, `"ffmpeg"`, `"ffprobe"`, `"soffice"`, or one named in `[recipes] programs`) |

**Text** (regular expressions of the `regex` crate, which answer in a time proportional to the
text, whatever the pattern; Lua's own patterns are not there):

| | |
|---|---|
| `rada.match(text, pattern)` | the groups of the first match (the whole match when there are none), or `nil` |
| `rada.replace(text, pattern, with)` | the text with every match replaced (`$1`, `${name}` are the groups), and how many |
| `rada.split(text, pattern)` | the pieces between the matches |
| `rada.join(a, b, …)`, `rada.parent(p)`, `rada.name(p)`, `rada.stem(p)`, `rada.ext(p)` | paths as text: nothing is looked at on the disk |

**Ask** (each adds a request to the plan, and does nothing else; paths are whole paths):

| | |
|---|---|
| `rada.copy(sources, folder [, { conflict, verify }])`, `rada.move(…)` | `conflict` is `"skip"` (the default) or `"keep_both"`: a recipe never replaces a file |
| `rada.trash(paths)` | to the trash, from where undo brings them back |
| `rada.mkdir(path)` | a new folder |
| `rada.rename(path, new_name [, { conflict }])` | a new name in the same folder |
| `rada.checksum { input, output }` | the SHA-256 of `input`, written to `output` as `sha256sum` does |
| `rada.run { program, args, inputs, output, collect, timeout, ok_exit, size_hint }` | one new file, `output`, made by a program from `inputs` |

In `rada.run`, `args` is a list of texts and of these words, which rada fills in when the
program runs: `rada.input(n)` (the n-th input, from 1), `rada.inputs` (all of them, one argument
each), `rada.out` (the file the program is to write), `rada.out_after("-sOutputFile=")` (that
text with the file right after it, as one argument), `rada.outdir` (the folder it works in, for
a program that names its file by itself; then `collect = { single = "pdf" }` says which file to
take), `rada.profile` (LibreOffice's option for a profile of its own). `chain = { { program,
args }, … }` runs several programs one after the other, each reading what the one before wrote
(`rada.prev`). `timeout` is in seconds (600 by default), `ok_exit` the exit codes that mean
success (`{ 0 }`).

`rada.log(…)` writes a line to rada's log (`log/rada.log.<date>` in its state folder; at most 200 lines a run). `rada.api` is the
version of all this: `1`.

## What a recipe does not have

No `io`, `os`, `package`, `require`, `debug`, `load`, `loadfile`, `dofile`, `collectgarbage`,
`print`, `coroutine`, `xpcall`; no `string.find`, `match`, `gmatch`, `gsub` (one call of a Lua
pattern can take hours, and nothing can interrupt it) or `string.dump`; no `math.random`. A file
of compiled Lua is not read. What is there: `assert`, `error`, `ipairs`, `pairs`, `next`,
`pcall`, `select`, `tonumber`, `tostring`, `type`, the `raw…` functions, `setmetatable` (without
`__gc`), `getmetatable`, the rest of `string`, `math`, `table` and `utf8`.

## Limits

- Each time rada asks a recipe something (read its file, ask `available`, run `run`) it is a new
  Lua state: nothing is kept from one time to the next.
- `run` may take 100 million instructions and 30 seconds; reading the file and `available`, 5
  million and 2 seconds. Memory: 32 MB. Past a limit the recipe is stopped and the plan is not
  made; `pcall` cannot catch it.
- A file of recipes is at most 64 KB of UTF-8 text; at most 100 files are read; a run asks for
  at most 10 000 things.
- `pairs` goes through a table in a fixed order (numbers, then texts, then the rest), so the same
  recipe on the same files gives the same plan. `next` keeps Lua's own order, which changes from
  one run to the next; so does what `tostring` says of a table.
- `[lua] enabled = false` in `config.toml`, or `rada --no-lua`, turns recipes off, the actions
  with them; rada built without the `lua` feature has none.

## What rada guarantees

- **A recipe cannot change a file.** It has nothing that writes, and what it asks is only
  shown: the plan window lists every file read, every program with the file that will run, the
  one file each will write, and nothing happens unless you confirm it.
- **What it asks is done like everything else**: in the journal, with one `Ctrl+Z` for the
  whole plan; a new file is never put over another one; the inputs are checked before and after
  a program runs; undo takes away a new file only if it has not changed since. See
  [SAFETY.md](SAFETY.md#actions-that-run-another-program) for what rada guarantees about a
  program it runs, and what it does not (anything the program does besides writing its file).
- **The interface does not wait for it**: the recipe runs on a job, and `Esc` stops it.
