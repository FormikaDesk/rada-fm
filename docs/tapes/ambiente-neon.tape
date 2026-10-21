# The terminal and the environment of the recordings on the "Neon" theme, in the panels view.
Set Shell "bash"
Set FontFamily "JetBrainsMono Nerd Font"
Set FontSize 14
Set Padding 14
# The size of the terminal (170x42 cells at this font) is set by each tape, before Source:
# Set Width 1584 / Set Height 800.
Set Theme { "name": "Neon", "black": "#0e1022", "red": "#ff6b8a", "green": "#5fe3a1", "yellow": "#ffd866", "blue": "#6fa8ff", "magenta": "#8b7cf6", "cyan": "#4fe0f0", "white": "#d9ddf2", "brightBlack": "#7b82a9", "brightRed": "#ff6b8a", "brightGreen": "#5fe3a1", "brightYellow": "#ffd866", "brightBlue": "#6fa8ff", "brightMagenta": "#8b7cf6", "brightCyan": "#4fe0f0", "brightWhite": "#ffffff", "background": "#0e1022", "foreground": "#d9ddf2", "selection": "#2a2d52", "cursor": "#8b7cf6" }
Set TypingSpeed 70ms
Set Framerate 24

# Everything below is typed where nobody sees it: the variables point rada at the demo home
# ($DEMO_HOME, made by scripts/demo-home), in Italian, with true colour and no network.
Hide
Type "export PS1='$ ' TERM=xterm-256color COLORTERM=truecolor LANG=it_IT.UTF-8 LC_ALL=it_IT.UTF-8"
Enter
Type "export HOME=$DEMO_HOME XDG_CONFIG_HOME=$DEMO_HOME/.config XDG_DATA_HOME=$DEMO_HOME/.local/share XDG_DATA_DIRS=$DEMO_HOME/.local/share XDG_STATE_HOME=$DEMO_HOME/.local/state XDG_CACHE_HOME=$DEMO_HOME/.cache"
Enter
Type `sed -i 's/^layout = .*/layout = "panels"/' $DEMO_HOME/.config/rada/config.toml`
Enter
Type "clear"
Enter
