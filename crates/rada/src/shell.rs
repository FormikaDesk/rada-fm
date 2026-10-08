//! Shell helpers that make the shell follow rada's last folder (`v` instead of `rada`).

pub fn snippet(shell: &str) -> &'static str {
    match shell {
        "fish" => FISH,
        "nushell" => NUSHELL,
        "powershell" => POWERSHELL,
        _ => POSIX,
    }
}

const POSIX: &str = r#"# rada: `v` opens rada and cds to where you left it. Add to your rc file:
#   eval "$(rada --init bash)"      # or zsh
v() {
  local tmp d
  tmp="$(mktemp -t rada-cwd.XXXXXX)" || return
  rada --cwd-file "$tmp" "$@"
  if [ -s "$tmp" ]; then
    d="$(cat -- "$tmp")"
    [ -n "$d" ] && [ "$d" != "$PWD" ] && cd -- "$d"
  fi
  rm -f -- "$tmp"
}
"#;

const FISH: &str = r#"# rada: `v` opens rada and cds to where you left it. Add to config.fish:
#   rada --init fish | source
function v
    set -l tmp (mktemp -t rada-cwd.XXXXXX)
    rada --cwd-file $tmp $argv
    if test -s $tmp
        set -l d (cat $tmp)
        if test -n "$d" -a "$d" != "$PWD"
            cd -- $d
        end
    end
    rm -f -- $tmp
end
"#;

const NUSHELL: &str = r#"# rada: `v` opens rada and cds to where you left it. Save as a script and `source` it.
def --env v [...args] {
  let tmp = (mktemp -t rada-cwd.XXXXXX)
  ^rada --cwd-file $tmp ...$args
  let d = (open --raw $tmp | str trim)
  if ($d | is-not-empty) and ($d != $env.PWD) { cd $d }
  rm -f $tmp
}
"#;

const POWERSHELL: &str = r#"# rada: `v` opens rada and cds to where you left it. Add to your $PROFILE:
function v {
  $tmp = New-TemporaryFile
  rada --cwd-file $tmp.FullName @args
  $d = (Get-Content -Raw $tmp.FullName).Trim()
  if ($d -and $d -ne $PWD.Path) { Set-Location -LiteralPath $d }
  Remove-Item $tmp.FullName
}
"#;
