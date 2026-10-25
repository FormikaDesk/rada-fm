# Tapes

The screenshots and the GIFs of the README are drawn from these tapes with
[VHS](https://github.com/charmbracelet/vhs), on a home folder of made-up files
(`scripts/demo-home`): nothing in them comes from the machine they were made on.

```sh
scripts/docs-media            # everything
scripts/docs-media principale # the tapes whose name starts with it
```

The terminal is 170×42 cells in JetBrainsMono Nerd Font. `principale-due-riquadri` is 78×45 (`Set Width 760`, `Set Height 858`) to show the two-box panels view. Each tape starts from
`ambiente.tape` (the terminal and the environment: language, a home of its own, no network)
`ambiente-carta.tape` (the same on the light "Carta" theme, `--theme carta`; `recenti` and `recenti-carta` draw the Recent list), `ambiente-gesso.tape` (the same on the default light theme, "Gesso", `--theme rada-light`: `principale-gesso`, `tag-menu-gesso`, `recenti-gesso`, `notifiche-gesso`) `ambiente-neon.tape` and `ambiente-gesso-pannelli.tape` (the panels view on Neon and on Gesso: `principale-pannelli`, `principale-pannelli-gesso`; they ask for `layout = "panels"` in the demo home's configuration, which otherwise keeps the classic layout) or `ambiente-catppuccin.tape` and ends in a PNG of
`docs/screenshots/` or a GIF of `docs/media/`. VHS, `ttyd`, `ffmpeg`, python3 with Pillow and
a JetBrainsMono Nerd Font have to be installed; nothing needs `sudo`.

VHS takes a screenshot of nothing now and then, so `scripts/docs-media` tries a tape up to six times
and stops if it still made none of what it promises. The variables that name the terminal it is
run from (`TERM_PROGRAM`, `GHOSTTY_*`, `RADA_*`…) are unset first: rada decides what to draw from them.

Three images do **not** come from these tapes: `docs/screenshots/panels-neon.png`, `docs/screenshots/classic.png` and `docs/media/demo-panels.gif`. They were taken in a real Ghostty window, because VHS's terminal does not draw images: the ring of the SPACE box becomes a bar and the pictures in the preview become blocks, so a VHS picture is not what rada looks like in a terminal that can draw them. They were made on a home folder of `scripts/demo-home` with the Italian names changed to English (made-up files, nothing from the machine), with `--demo vetrina` for the panels, `--demo main` for the classic one and, for the GIF, keys like those of `demo.tape` sent to the window (plus `c` in the plan, to choose Keep both).
