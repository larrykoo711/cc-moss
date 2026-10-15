# Modern tools get NEW names; ls, cat, grep, find and cd keep their standard
# behaviour. rg rejects most grep flags, so an aliased grep works when typed
# and breaks in scripts; an aliased cd turns "no such directory" into a jump
# somewhere else.
alias ll='ls -l'
alias lla='ls -la'
alias grep='grep --color=auto'
alias psg='ps aux | grep'
# gl is read-only here: some frameworks map it to `git pull`, which writes.
alias gl='git log --oneline --graph --decorate'
# Every modern tool sits behind a check: a missing tool costs one alias, not
# an error on every shell start. Debian names bat `batcat` and fd `fdfind`.
if command -v eza >/dev/null 2>&1; then
    alias lg='eza -la --icons --group-directories-first --git'
    alias lt='eza --tree --level=2 --icons'
fi
if command -v bat >/dev/null 2>&1; then
    alias catp='bat'; alias catn='bat --paging=never'
elif command -v batcat >/dev/null 2>&1; then
    alias catp='batcat'; alias catn='batcat --paging=never'
fi
if command -v fd >/dev/null 2>&1; then alias ff='fd'
elif command -v fdfind >/dev/null 2>&1; then alias ff='fdfind'; fi
command -v procs >/dev/null 2>&1 && alias psp='procs --tree'
