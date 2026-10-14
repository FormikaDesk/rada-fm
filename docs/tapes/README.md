# Tapes

The screenshots and the GIFs of the README are drawn from these tapes with
[VHS](https://github.com/charmbracelet/vhs), on a home folder of made-up files
(`scripts/demo-home`): nothing in them comes from the machine they were made on.

```sh
scripts/docs-media            # everything
scripts/docs-media principale # the tapes whose name starts with it
```

The terminal is 170×42 cells in JetBrainsMono Nerd Font. Each tape starts from
`ambiente.tape` (the terminal and the environment: language, a home of its own, no network)
`ambiente-carta.tape` (the same on the light "Carta" theme) or `ambiente-catppuccin.tape` and ends in a PNG of
`docs/screenshots/` or a GIF of `docs/media/`. VHS, `ttyd`, `ffmpeg`, python3 with Pillow and
a JetBrainsMono Nerd Font have to be installed; nothing needs `sudo`.

VHS takes a screenshot of nothing now and then, so `scripts/docs-media` tries a tape up to six times
and stops if it still made none of what it promises. The variables that name the terminal it is
run from (`TERM_PROGRAM`, `GHOSTTY_*`, `RADA_*`…) are unset first: rada decides what to draw from them.
