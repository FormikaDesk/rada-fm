//! Shell helpers that make the shell follow vela's last folder (`v` instead of `vela`).

pub fn snippet(shell: &str) -> &'static str {
    match shell {
        "fish" => FISH,
        "nushell" => NUSHELL,
        "powershell" => POWERSHELL,
        _ => POSIX,
    }
}

const POSIX: &str = r#"# vela: `v` opens vela and cds to where you left it. Add to your rc file:
#   eval "$(vela --init bash)"      # or zsh
v() {
  local tmp d
  tmp="$(mktemp -t vela-cwd.XXXXXX)" || return
  vela --cwd-file "$tmp" "$@"
  if [ -s "$tmp" ]; then
    d="$(cat -- "$tmp")"
    [ -n "$d" ] && [ "$d" != "$PWD" ] && cd -- "$d"
  fi
  rm -f -- "$tmp"
}
"#;

const FISH: &str = r#"# vela: `v` opens vela and cds to where you left it. Add to config.fish:
#   vela --init fish | source
function v
    set -l tmp (mktemp -t vela-cwd.XXXXXX)
    vela --cwd-file $tmp $argv
    if test -s $tmp
        set -l d (cat $tmp)
        if test -n "$d" -a "$d" != "$PWD"
            cd -- $d
        end
    end
    rm -f -- $tmp
end
"#;

const NUSHELL: &str = r#"# vela: `v` opens vela and cds to where you left it. Save as a script and `source` it.
def --env v [...args] {
  let tmp = (mktemp -t vela-cwd.XXXXXX)
  ^vela --cwd-file $tmp ...$args
  let d = (open --raw $tmp | str trim)
  if ($d | is-not-empty) and ($d != $env.PWD) { cd $d }
  rm -f $tmp
}
"#;

const POWERSHELL: &str = r#"# vela: `v` opens vela and cds to where you left it. Add to your $PROFILE:
function v {
  $tmp = New-TemporaryFile
  vela --cwd-file $tmp.FullName @args
  $d = (Get-Content -Raw $tmp.FullName).Trim()
  if ($d -and $d -ne $PWD.Path) { Set-Location -LiteralPath $d }
  Remove-Item $tmp.FullName
}
"#;
