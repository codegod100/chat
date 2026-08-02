# Chat
#   nix develop / nix run
#   just apk-release          # phone aarch64 release APK
#   just publish-apk          # push to https://chat-apk.boxd.sh

set shell := ["bash", "-euo", "pipefail", "-c"]

default:
    @just --list

run:
    nix run

build:
    nix run .#build

# Android APK via cargo-apk (needs NDK + rustup android target)
apk *args:
    ./scripts/build-apk.sh {{args}}

apk-release:
    ./scripts/build-apk.sh --release --target aarch64-linux-android

apk-release-x86:
    ./scripts/build-apk.sh --release --target x86_64-linux-android

publish-apk:
    ./scripts/publish-apk.sh
