# The menu of the right button on a file among the last rows of the list: it stays above the
# status bar and the key hints.
Set Width 1584
Set Height 800
Source "docs/tapes/ambiente.tape"
Output "docs/media/.scarto/menu-fondo.gif"
Type "rada --icons nerd --images halfblocks --demo menubottom ~/projects/demo"
Enter
Sleep 2s
Show
Sleep 2500ms
Screenshot "docs/screenshots/menu-fondo.png"
Sleep 1s
