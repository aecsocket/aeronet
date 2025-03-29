FROM archlinux:base-devel

# setup dev user
RUN useradd -m -s /usr/bin/fish dev && \
    echo "dev ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/dev
USER dev
RUN mkdir -p "$HOME/.vscode-server"

# setup dev tools
RUN sudo pacman -Syu --noconfirm \
    bat exa fish fzf git gnupg helix just less man-db openssh ripgrep starship typos unzip yq zellij zoxide && \
    git clone https://aur.archlinux.org/yay-bin.git /tmp/yay && \
    cd /tmp/yay && \
    makepkg -si --noconfirm
COPY config.fish /etc/fish/config.fish
ENV EDITOR=/usr/bin/helix

# install Rust
RUN yay -Syu --noconfirm \
    rustup clang lld taplo && \
    rustup default stable && \
    rustup install nightly && \
    rustup target add x86_64-unknown-none && \
    rustup target add thumbv6m-none-eabi && \
    curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | sh && \
    cargo binstall cargo-shear

# install Rust fuzzing tools
RUN cargo binstall cargo-fuzz

# install Bevy libraries
RUN yay -Syu --noconfirm alsa-lib libxkbcommon vulkan-tools mesa vulkan-radeon

# install Steam libraries
RUN echo -e "[multilib]\nInclude = /etc/pacman.d/mirrorlist" | sudo tee -a /etc/pacman.conf && \
    yay -Syu --noconfirm steam

# install WASM tools
RUN cargo binstall wasm-server-runner
