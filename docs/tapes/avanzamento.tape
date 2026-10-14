# Progress counted in bytes: two files with real data copied slowly (RADA_DEMO_SLOW_MBPS, a
# developer aid that throttles the copy) so that the bar is there to be drawn.
Set Width 1584
Set Height 800
Source "docs/tapes/ambiente.tape"
Output "docs/media/.scarto/avanzamento.gif"
Type "RADA_DEMO_SLOW_MBPS=0.5 rada --icons nerd --images halfblocks --demo main ~/projects/demo"
Enter
Sleep 2s
Show
Down 5
Space
Space
Sleep 400ms
Ctrl+C
Sleep 300ms
Ctrl+L
Type "~/Documenti"
Enter
Sleep 1s
Ctrl+V
Sleep 1500ms
Enter
Sleep 6s
Screenshot "docs/screenshots/avanzamento.png"
Sleep 1s
