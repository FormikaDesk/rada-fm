# The same screen in English: the language follows the system, or `language = "en"`.
Set Width 1584
Set Height 800
Source "docs/tapes/ambiente.tape"
Output "docs/media/.scarto/principale-inglese.gif"
Type "export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8"
Enter
Type "rada --icons nerd --images halfblocks --demo main ~/projects/demo"
Enter
Sleep 2s
Show
Sleep 2500ms
Screenshot "docs/screenshots/principale-inglese.png"
