# A remote tmux with `mouse on` switches THIS terminal into mouse-reporting
# mode. When the connection drops, tmux never switches it back and every mouse
# move arrives as keyboard input (`35;57;18M` spam that Ctrl-C cannot stop).
# So reset those modes whenever ssh exits. Same arguments, same exit code;
# scripts still get the real binary (functions are not inherited by
# non-interactive shells).
ssh() {
    command ssh "$@"
    local rc=$?
    # mouse x4, focus events, bracketed paste, cursor visible, leave alt screen
    [[ -t 1 ]] && printf '\e[?1000l\e[?1002l\e[?1003l\e[?1006l\e[?1004l\e[?2004l\e[?25h\e[?1049l'
    return $rc
}
