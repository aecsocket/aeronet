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
# RUN yay -Syu --noconfirm alsa-lib libxkbcommon vulkan-tools mesa vulkan-radeon
RUN yay -Syu --noconfirm alsa-lib libxkbcommon vulkan-tools && \
    # install mesa 24.3.4 because 25.0 has a regression on AMD Radeon 780M where conformanceVersion reports as 0.0.0.0,
    # making it fail with wgpu adapter initialization
    # <https://github.com/warpdotdev/Warp/issues/6015>
    sudo pacman -U --noconfirm https://archive.archlinux.org/packages/m/mesa/mesa-1%3A24.3.4-1-x86_64.pkg.tar.zst && \
    sudo pacman -U --noconfirm https://archive.archlinux.org/packages/v/vulkan-radeon/vulkan-radeon-1%3A24.3.4-1-x86_64.pkg.tar.zst
