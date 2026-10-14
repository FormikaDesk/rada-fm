# The same terminal and environment on the light "Carta" theme.
Set Shell "bash"
Set FontFamily "JetBrainsMono Nerd Font"
Set FontSize 14
Set Padding 14
# The size of the terminal (170x42 cells at this font) is set by each tape, before Source:
# Set Width 1584 / Set Height 800.
Set Theme { "name": "Carta", "black": "#3a3229", "red": "#a8454c", "green": "#4b7438", "yellow": "#8c5f16", "blue": "#405c84", "magenta": "#77559e", "cyan": "#2c6e7d", "white": "#ebe4d5", "brightBlack": "#625a4d", "brightRed": "#a8454c", "brightGreen": "#4b7438", "brightYellow": "#8c5f16", "brightBlue": "#3b659f", "brightMagenta": "#77559e", "brightCyan": "#2c6e7d", "brightWhite": "#f4ead6", "background": "#ebe4d5", "foreground": "#3a3229", "selection": "#ddd6c7", "cursor": "#405c84" }
Set TypingSpeed 70ms
Set Framerate 24

Hide
Type "export PS1='$ ' TERM=xterm-256color COLORTERM=truecolor LANG=it_IT.UTF-8 LC_ALL=it_IT.UTF-8"
Enter
Type "export HOME=$DEMO_HOME XDG_CONFIG_HOME=$DEMO_HOME/.config XDG_DATA_HOME=$DEMO_HOME/.local/share XDG_DATA_DIRS=$DEMO_HOME/.local/share XDG_STATE_HOME=$DEMO_HOME/.local/state XDG_CACHE_HOME=$DEMO_HOME/.cache"
Enter
Type "clear"
Enter
