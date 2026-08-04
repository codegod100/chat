#!/usr/bin/env bash
# Install Android NDK/SDK + cargo-apk for CI (ubuntu-latest).
set -euo pipefail

export PATH="${HOME}/.cargo/bin:${PATH}"

NDK="${ANDROID_NDK_HOME:-$HOME/.local/share/android-ndk-r29}"
SDK="${ANDROID_HOME:-$HOME/.local/share/android-sdk}"

need() { command -v "$1" >/dev/null || { echo "missing: $1" >&2; exit 1; }; }
need curl
need unzip
need java
need rustc
need cargo

if [[ ! -d "$NDK" ]]; then
  echo "Installing Android NDK r29 → $NDK" >&2
  mkdir -p "$(dirname "$NDK")"
  curl -fsSL -o /tmp/android-ndk-r29.zip \
    "https://dl.google.com/android/repository/android-ndk-r29-linux.zip"
  unzip -q /tmp/android-ndk-r29.zip -d "$(dirname "$NDK")"
  rm -f /tmp/android-ndk-r29.zip
fi

if [[ ! -d "$SDK/cmdline-tools/latest" ]]; then
  echo "Installing Android SDK → $SDK" >&2
  mkdir -p "$SDK/cmdline-tools/latest"
  curl -fsSL -o /tmp/cmdline-tools.zip \
    "https://dl.google.com/android/repository/commandlinetools-linux-11076708_latest.zip"
  unzip -q /tmp/cmdline-tools.zip -d /tmp/cmdline-tools-extract
  mv /tmp/cmdline-tools-extract/cmdline-tools/* "$SDK/cmdline-tools/latest/"
  rm -rf /tmp/cmdline-tools-extract /tmp/cmdline-tools.zip
fi

export ANDROID_NDK_HOME="$NDK"
export ANDROID_NDK_ROOT="$NDK"
export ANDROID_HOME="$SDK"
export PATH="$SDK/cmdline-tools/latest/bin:$SDK/platform-tools:$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin:$PATH"

yes | sdkmanager --licenses >/dev/null 2>&1 || true
sdkmanager "platform-tools" "build-tools;34.0.0" "platforms;android-34" >/dev/null

if ! command -v cargo-apk >/dev/null; then
  echo "Installing cargo-apk" >&2
  cargo install cargo-apk --locked
fi

rustup target add aarch64-linux-android

echo "Android CI toolchain ready" >&2
echo "  NDK=$NDK" >&2
echo "  SDK=$SDK" >&2
command -v cargo-apk >&2
command -v apksigner >&2 || true
