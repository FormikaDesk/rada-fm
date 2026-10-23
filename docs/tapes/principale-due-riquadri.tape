# The panels view in a window of 78x45 cells: two boxes (the folder and NAV), as the window is too
# narrow for three. Same state as principale-pannelli.
Set Width 760
Set Height 858
Source "docs/tapes/ambiente-neon.tape"
Output "docs/media/.scarto/principale-due-riquadri.gif"
Type "rada --icons nerd --images halfblocks --theme neon --demo vetrina ~/projects/demo"
Enter
Sleep 2s
Show
Sleep 2500ms
Screenshot "docs/screenshots/principale-due-riquadri.png"
Sleep 1s
