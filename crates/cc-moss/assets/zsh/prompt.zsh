# starship must stay last: anything after it that touches PROMPT overrides it.
# Without starship installed this does nothing instead of erroring.
command -v starship >/dev/null 2>&1 && eval "$(starship init zsh)"
