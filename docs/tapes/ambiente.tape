# The terminal and the environment of the recordings (dark theme).
Set Shell "bash"
Set FontFamily "JetBrainsMono Nerd Font"
Set FontSize 14
Set Padding 14
# The size of the terminal (170x42 cells at this font) is set by each tape, before Source:
# Set Width 1584 / Set Height 800.
Set Theme { "name": "Rada", "black": "#15161e", "red": "#f7768e", "green": "#9ece6a", "yellow": "#e0af68", "blue": "#86a0d4", "magenta": "#bb9af7", "cyan": "#7dcfff", "white": "#a9b1d6", "brightBlack": "#414868", "brightRed": "#f7768e", "brightGreen": "#9ece6a", "brightYellow": "#e0af68", "brightBlue": "#86a0d4", "brightMagenta": "#bb9af7", "brightCyan": "#7dcfff", "brightWhite": "#c8cde0", "background": "#1b1d27", "foreground": "#c8cde0", "selection": "#282c3a", "cursor": "#c8cde0" }
Set TypingSpeed 70ms
Set Framerate 24

# Everything below is typed where nobody sees it: the variables point rada at the demo home
# ($DEMO_HOME, made by scripts/demo-home), in Italian, with true colour and no network.
Hide
Type "export PS1='$ ' TERM=xterm-256color COLORTERM=truecolor LANG=it_IT.UTF-8 LC_ALL=it_IT.UTF-8"
Enter
Type "export HOME=$DEMO_HOME XDG_CONFIG_HOME=$DEMO_HOME/.config XDG_DATA_HOME=$DEMO_HOME/.local/share XDG_DATA_DIRS=$DEMO_HOME/.local/share XDG_STATE_HOME=$DEMO_HOME/.local/state XDG_CACHE_HOME=$DEMO_HOME/.cache"
Enter
Type "clear"
Enter
