FROM debian:bookworm

# Install system dependencies
RUN apt-get update && apt-get install -y \
    # Sway and Wayland
    sway \
    grim \
    # Vulkan software rendering (lavapipe)
    mesa-vulkan-drivers \
    # Wayland development libraries
    libwayland-dev \
    libxkbcommon-dev \
    # Fonts
    fonts-dejavu-core \
    # Utilities
    procps \
    sudo \
    curl \
    build-essential \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Install Rust via rustup (get the latest stable)
ENV RUSTUP_HOME=/usr/local/rustup
ENV CARGO_HOME=/usr/local/cargo
ENV PATH=/usr/local/cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable

# Create non-root user (Sway refuses to run as root)
RUN useradd -m -s /bin/bash testuser && \
    echo "testuser ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers

# Set environment for headless Sway with software rendering
ENV WLR_BACKENDS=headless
ENV WLR_LIBINPUT_NO_DEVICES=1
ENV MESA_LOADER_DRIVER_OVERRIDE=lvp
ENV VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json
ENV XDG_RUNTIME_DIR=/tmp/runtime
ENV WAYLAND_DISPLAY=wayland-1
ENV HOME=/home/testuser
ENV CARGO_HOME=/home/testuser/.cargo

# Create runtime directory with correct permissions
RUN mkdir -p /tmp/runtime && chmod 700 /tmp/runtime && chown testuser:testuser /tmp/runtime

# Set up cargo directory for testuser
RUN mkdir -p /home/testuser/.cargo && chown -R testuser:testuser /home/testuser/.cargo

# Set up working directory
WORKDIR /workspace

# Copy sway config
COPY sway-config /etc/sway/config

# Copy entrypoint
COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

# Switch to non-root user
USER testuser

ENTRYPOINT ["/entrypoint.sh"]
