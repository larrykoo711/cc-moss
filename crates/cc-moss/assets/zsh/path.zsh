# Non-interactive shells (`ssh host cmd`, cron, scripts, agents) read only
# ~/.zshenv, not ~/.zprofile or ~/.zshrc. Without this, tools installed with
# Homebrew or into ~/.local/bin look missing there. PATH only: nothing prints.
if [ -x /opt/homebrew/bin/brew ]; then eval "$(/opt/homebrew/bin/brew shellenv)"
elif [ -x /usr/local/bin/brew ]; then eval "$(/usr/local/bin/brew shellenv)"
elif [ -x /home/linuxbrew/.linuxbrew/bin/brew ]; then eval "$(/home/linuxbrew/.linuxbrew/bin/brew shellenv)"; fi
for _cc_moss_dir in "$HOME/.local/bin" "$HOME/.cargo/bin"; do
    case ":$PATH:" in *":$_cc_moss_dir:"*) ;; *) [ -d "$_cc_moss_dir" ] && PATH="$_cc_moss_dir:$PATH" ;; esac
done
unset _cc_moss_dir
export PATH
