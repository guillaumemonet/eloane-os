# SPDX-License-Identifier: GPL-3.0-or-later
# Eloane - image de build : archiso + toolchain Rust (via rustup, pour respecter
# rust-toolchain.toml a la racine du repo).
#
#   docker build -t eloane-builder build/
#   docker run --rm --privileged -v "<repo>:/src" eloane-builder /src/build/build-iso.sh
FROM archlinux:latest

# multilib n'est pas necessaire cote builder : mkarchiso utilise image/pacman.conf.
RUN pacman -Syu --noconfirm --needed \
        archiso \
        base-devel \
        git \
        rustup \
        mtools \
        dosfstools \
        squashfs-tools \
        erofs-utils \
        libisoburn \
    && pacman -Scc --noconfirm

# Toolchain Rust installee dans l'image (RUSTUP_HOME fige) ; le registre cargo
# (CARGO_HOME) et le dossier target sont des volumes nommes montes au run.
ENV RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/cargo \
    CARGO_TARGET_DIR=/build/target
RUN rustup toolchain install stable --profile minimal -c rustfmt,clippy \
    && rustup default stable \
    && rustc --version && cargo --version

WORKDIR /src
CMD ["/src/build/build-iso.sh"]
