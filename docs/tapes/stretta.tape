# A narrow terminal: the sidebar is a column of icons and the details pane steps aside.
Set Width 880
Set Height 620
Source "docs/tapes/ambiente.tape"
Output "docs/media/.scarto/stretta.gif"
Type "rada --icons nerd --images halfblocks --demo main ~/projects/demo"
Enter
Sleep 2s
Show
Sleep 800ms
Escape
Down 3
Sleep 2500ms
Screenshot "docs/screenshots/stretta.png"
