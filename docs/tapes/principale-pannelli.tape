# The panels view on the "Neon" theme: the same state as the main screen (one item selected, two
# waiting to be pasted, the cursor on a picture, the card of the last copy), in boxes.
Set Width 1584
Set Height 800
Source "docs/tapes/ambiente-neon.tape"
Output "docs/media/.scarto/principale-pannelli.gif"
Type "rada --icons nerd --images halfblocks --theme neon --demo vetrina ~/projects/demo"
Enter
Sleep 2s
Show
Sleep 2500ms
Screenshot "docs/screenshots/principale-pannelli.png"
Sleep 1s
