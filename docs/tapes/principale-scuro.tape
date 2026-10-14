# The main screen, dark theme: one item selected, two waiting to be pasted, the cursor on a
# picture and the card of the last copy.
Set Width 1584
Set Height 800
Source "docs/tapes/ambiente.tape"
Output "docs/media/.scarto/principale-scuro.gif"
Type "rada --icons nerd --images halfblocks --demo vetrina ~/projects/demo"
Enter
Sleep 2s
Show
Sleep 2500ms
Screenshot "docs/screenshots/principale-scuro.png"
Sleep 1s
