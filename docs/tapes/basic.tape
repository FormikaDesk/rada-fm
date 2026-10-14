# The basic profile, for a terminal that is no more than a vt100: ASCII, 16 colours.
Set Width 1584
Set Height 800
Source "docs/tapes/ambiente.tape"
Output "docs/media/.scarto/basic.gif"
Type "TERM=vt100 rada --demo vetrina ~/projects/demo"
Enter
Sleep 2s
Show
Sleep 2500ms
Screenshot "docs/screenshots/basic.png"
Sleep 1s
