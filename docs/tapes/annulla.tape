# The window of an undo: a copy was made, then Ctrl+Z asks what taking it back will do.
Set Width 1584
Set Height 800
Source "docs/tapes/ambiente.tape"
Output "docs/media/.scarto/annulla.gif"
Type "rada --icons nerd --images halfblocks --demo main ~/projects/demo"
Enter
Sleep 2s
Show
Down 5
Space
Space
Space
Sleep 400ms
Ctrl+C
Sleep 300ms
Ctrl+L
Type "~/Documenti"
Enter
Sleep 800ms
Ctrl+V
Sleep 1s
Right
Sleep 300ms
Enter
Sleep 1500ms
Escape
Sleep 700ms
Ctrl+Z
Sleep 2500ms
Screenshot "docs/screenshots/annulla.png"
Sleep 1s
