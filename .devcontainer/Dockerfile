FROM archlinux:base-devel

RUN pacman -Syu --noconfirm \
    git openssh gnupg fish starship ripgrep fzf bat exa zoxide yq && \
    chsh -s /usr/bin/fish root && \
    mkdir -p "$HOME/.config/fish" && \
    echo "
set fish_greeting
starship init fish | source
" >> "$HOME/.config/fish/config.fish"

RUN pacman -Syu --noconfirm \
    rustup pkgconf alsa-lib libxkbcommon vulkan-radeon && \
    rustup default stable && \
    rustup install nightly && \
    curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | sh && \
    cargo binstall cargo-shear
