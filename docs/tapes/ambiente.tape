# The terminal and the environment of the recordings (dark theme).
Set Shell "bash"
Set FontFamily "JetBrainsMono Nerd Font"
Set FontSize 14
Set Padding 14
Set Width 1500
Set Height 836
Set Theme "TokyoNight"
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
