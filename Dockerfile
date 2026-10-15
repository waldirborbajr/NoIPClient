# Cross-compile noip-client for Android (Termux) without a host toolchain.
#
# Build the arm64 binary (default) and export it to ./dist:
#   docker build --target artifact --output type=local,dest=dist .
#
# Other targets:
#   docker build --target artifact --build-arg TARGET=armv7-linux-androideabi \
#     --output type=local,dest=dist .

FROM rust:1.90-bookworm AS builder

ARG NDK_VERSION=r27c
ARG API_LEVEL=24
ARG TARGET=aarch64-linux-android

ENV ANDROID_NDK_HOME=/opt/android-ndk
ENV PATH=${ANDROID_NDK_HOME}/toolchains/llvm/prebuilt/linux-x86_64/bin:${PATH}

RUN apt-get update \
 && apt-get install -y --no-install-recommends curl unzip ca-certificates file \
 && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL -o /tmp/ndk.zip \
      "https://dl.google.com/android/repository/android-ndk-${NDK_VERSION}-linux.zip" \
 && unzip -q /tmp/ndk.zip -d /opt \
 && mv "/opt/android-ndk-${NDK_VERSION}" "${ANDROID_NDK_HOME}" \
 && rm /tmp/ndk.zip

RUN rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android

# Linker and C compiler per target. The `ring` crate (used by rustls) needs CC_*.
ENV CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=aarch64-linux-android${API_LEVEL}-clang \
    CC_aarch64_linux_android=aarch64-linux-android${API_LEVEL}-clang \
    AR_aarch64_linux_android=llvm-ar \
    CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_LINKER=armv7a-linux-androideabi${API_LEVEL}-clang \
    CC_armv7_linux_androideabi=armv7a-linux-androideabi${API_LEVEL}-clang \
    AR_armv7_linux_androideabi=llvm-ar \
    CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER=x86_64-linux-android${API_LEVEL}-clang \
    CC_x86_64_linux_android=x86_64-linux-android${API_LEVEL}-clang \
    AR_x86_64_linux_android=llvm-ar

WORKDIR /src
COPY . .

RUN cargo build --release --locked --target "${TARGET}" \
 && mkdir -p /out \
 && cp "target/${TARGET}/release/noipclient" /out/ \
 && cp noip.toml.example /out/ \
 && file "/out/noipclient"

# Export stage: only the binary and the example config.
FROM scratch AS artifact
COPY --from=builder /out/ /
