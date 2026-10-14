# The window that asks before something is deleted for good (it still wants the word typed).
Set Width 1584
Set Height 800
Source "docs/tapes/ambiente.tape"
Output "docs/media/.scarto/elimina.gif"
Type "RADA_DEMO_DEST=$HOME/Documenti rada --icons nerd --images halfblocks --demo delete ~/projects/demo"
Enter
Sleep 2s
Show
Sleep 1500ms
Type "si"
Sleep 1500ms
Screenshot "docs/screenshots/elimina.png"
Sleep 1s
