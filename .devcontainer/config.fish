set fish_greeting
starship init fish | source
zoxide init fish | source
set -U fish_user_paths "$HOME/.local/bin" "$HOME/.cargo/bin" $fish_user_paths

alias ls 'exa'
alias l. 'exa -a'
alias ll 'exa -l'
alias la 'exa -la'
alias cd 'z'
alias hx 'helix'
