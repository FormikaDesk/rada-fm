# Two cards piled: a copy that is running (slowly, so that it can be drawn) over one that is done.
Set Width 1584
Set Height 800
Source "docs/tapes/ambiente.tape"
Output "docs/media/.scarto/notifiche.gif"
Type "RADA_DEMO_SLOW_MBPS=0.5 RADA_DEMO_DEST=$HOME/Documenti rada --icons nerd --images halfblocks --demo toasts ~/projects/demo"
Enter
Sleep 2s
Show
Sleep 4s
Screenshot "docs/screenshots/notifiche.png"
Sleep 1s
