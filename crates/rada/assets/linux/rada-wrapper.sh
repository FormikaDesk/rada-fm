#!/usr/bin/env sh
# rada as the file chooser of xdg-desktop-portal-termfilechooser.
#
# The portal runs this script with six arguments and waits for it to end:
#   $1 multiple   1 when more than one file may be chosen
#   $2 directory  1 when a folder is wanted
#   $3 save       1 when this is "save as"
#   $4 path       the folder (or file) to start from; when saving, folder/suggested-name
#   $5 out        the file the answer goes to: one path per line, nothing at all if cancelled
#   $6 debug      1 to trace this script
#
# rada is started in a terminal window of its own (`rada setup file-picker` puts this script
# in place and writes the portal's configuration; see docs/desktop.md). Settings, from the
# portal's `env=` lines or your environment:
#   RADA_BIN   the rada program (default: rada)
#   TERMCMD    the terminal and its options, as the portal's other wrappers read it, for
#              example TERMCMD='foot --title "choose a file"'
#   TERMINAL   otherwise: the terminal to use; it must stay in front until rada ends
#              (a terminal that hands the work to a server and returns at once cannot be used)

multiple="$1"
directory="$2"
save="$3"
path="$4"
out="$5"
debug="$6"

set -e

if [ "$debug" = 1 ]; then
    set -x
fi

# The portal's service may start with a short PATH.
PATH="$PATH:$HOME/.local/bin:$HOME/.cargo/bin:/usr/local/bin"

rada="${RADA_BIN:-rada}"
class="com.formikadesk.rada.chooser"

if [ -z "$path" ]; then
    path="$HOME"
fi

# What rada is asked: the options first, then the folder to start in after `--`.
if [ "$save" = "1" ]; then
    # The path is folder/name: the folder to start in and the name to suggest.
    case "$path" in
    */) dir="$path"; name="" ;;
    */*) dir="${path%/*}"; name="${path##*/}" ;;
    *) dir="."; name="$path" ;;
    esac
    [ -n "$dir" ] || dir="/"
    set -- "$rada" --chooser-file "$out" "--chooser-save=$name" -- "$dir"
elif [ "$directory" = "1" ]; then
    if [ "$multiple" = "1" ]; then
        set -- "$rada" --chooser-file "$out" --chooser-dir --chooser-multiple -- "$path"
    else
        set -- "$rada" --chooser-file "$out" --chooser-dir -- "$path"
    fi
elif [ "$multiple" = "1" ]; then
    set -- "$rada" --chooser-file "$out" --chooser-multiple -- "$path"
else
    set -- "$rada" --chooser-file "$out" -- "$path"
fi

have() {
    command -v "$1" >/dev/null 2>&1
}

# The terminal: TERMCMD as it is, else $TERMINAL, else the first of the usual ones. For the
# ones rada knows, the options that give the window a name of its own (a rule can then make it
# float) and the ones that keep the program in front until the window is closed.
#
# TERMCMD (and TERMINAL) go through `eval` on purpose: they are a command line with its own
# quoting (`foot --title "choose a file"`), and that is how the other wrappers of
# xdg-desktop-portal-termfilechooser read it. They are the user's own configuration, set in the
# environment or in the portal's `env=` lines, like the script itself; nothing the portal is
# given by a program (the folder, the name to suggest) reaches the `eval`: those arrive as
# arguments and are only ever expanded as "$@".
title="rada: choose"
if [ -n "${TERMCMD:-}" ]; then
    eval "set -- $TERMCMD \"\$@\""
else
    term="${TERMINAL:-}"
    if [ -n "$term" ] && ! have "${term%% *}"; then
        term=""
    fi
    if [ -z "$term" ]; then
        for candidate in ghostty kitty foot alacritty wezterm konsole gnome-terminal xterm; do
            if have "$candidate"; then
                term="$candidate"
                break
            fi
        done
    fi
    if [ -z "$term" ]; then
        echo "rada-wrapper: no terminal found; set TERMCMD or TERMINAL" >&2
        exit 1
    fi
    first="${term%% *}"
    case "${first##*/}" in
    ghostty) flags="--gtk-single-instance=false --class=$class --title='$title' -e" ;;
    kitty) flags="--class $class --title '$title'" ;;
    foot) flags="--app-id=$class --title='$title'" ;;
    alacritty) flags="--class $class --title '$title' -e" ;;
    wezterm) flags="start --always-new-process --class $class --" ;;
    konsole) flags="--nofork -e" ;;
    gnome-terminal) flags="--wait --title='$title' --" ;;
    xterm) flags="-class $class -T '$title' -e" ;;
    *) flags="-e" ;;
    esac
    eval "set -- $term $flags \"\$@\""
fi

exec "$@"
