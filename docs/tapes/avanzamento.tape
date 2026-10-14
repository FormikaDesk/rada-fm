# Progress counted in bytes: a big file copied slowly (RADA_DEMO_SLOW_MBPS) so that the bar is
# there to be drawn.
Source docs/tapes/ambiente.tape
Output docs/media/.scarto/avanzamento.gif
Type "RADA_DEMO_SLOW_MBPS=24 rada --icons nerd --images halfblocks --demo main ~/projects/demo"
Enter
Sleep 2s
Show
Down 9
Sleep 300ms
Ctrl+C
Sleep 300ms
Ctrl+L
Type "~/Documenti"
Enter
Sleep 800ms
Ctrl+V
Sleep 1s
Enter
Sleep 1500ms
Screenshot docs/screenshots/avanzamento.png
Sleep 1s
