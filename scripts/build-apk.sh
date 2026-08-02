#!/usr/bin/env bash
# In-tree cargo-apk build for Chat.
#   ./scripts/build-apk.sh              # debug, aarch64
#   ./scripts/build-apk.sh --release    # release (signed)
#   ./scripts/build-apk.sh --release --target x86_64-linux-android
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="$ROOT/android"
TARGET="${CHAT_ANDROID_TARGET:-aarch64-linux-android}"
RELEASE=0

usage() {
  cat >&2 <<'EOF'
Usage: build-apk.sh [--release] [--target <triple>]

  --release   cargo apk build --release (needs signing metadata)
  --target    default aarch64-linux-android (phones); x86_64-linux-android for Waydroid

Env:
  ANDROID_NDK_HOME  default ~/.local/share/android-ndk-r29
  ANDROID_HOME      default ~/.local/share/android-sdk
EOF
  exit 2
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --release) RELEASE=1; shift ;;
    --target)
      [[ $# -ge 2 ]] || usage
      TARGET="$2"
      shift 2
      ;;
    -h|--help) usage ;;
    *)
      echo "unknown arg: $1" >&2
      usage
      ;;
  esac
done

export PATH="${HOME}/.cargo/bin:${PATH}"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$HOME/.local/share/android-ndk-r29}"
export ANDROID_NDK_ROOT="$ANDROID_NDK_HOME"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/.local/share/android-sdk}"
unset ANDROID_SDK_ROOT 2>/dev/null || true

# NixOS: SDK scripts shebang /bin/bash; wrap apksigner/zipalign for cargo-apk.
BT="$(ls -d "$ANDROID_HOME"/build-tools/*/ 2>/dev/null | sort -V | tail -1 || true)"
WRAP="$(mktemp -d)"
trap 'rm -rf "$WRAP"' EXIT
if [[ -n "$BT" && -d "$BT" ]]; then
  if [[ -x "${BT}zipalign" ]]; then
    ln -sf "${BT}zipalign" "$WRAP/zipalign"
  fi
  if [[ -f "${BT}lib/apksigner.jar" ]]; then
    cat >"$WRAP/apksigner" <<EOF
#!/usr/bin/env bash
exec java -jar "${BT}lib/apksigner.jar" "\$@"
EOF
    chmod +x "$WRAP/apksigner"
  fi
fi

export PATH="$WRAP:${ANDROID_NDK_HOME}/toolchains/llvm/prebuilt/linux-x86_64/bin:${ANDROID_HOME}/platform-tools:${PATH}"

need() { command -v "$1" >/dev/null || { echo "missing: $1" >&2; exit 1; }; }
need cargo
need cargo-apk
need rustc

[[ -d "$ANDROID_NDK_HOME" ]] || {
  echo "Set ANDROID_NDK_HOME=$ANDROID_NDK_HOME" >&2
  exit 1
}

case "$TARGET" in
  aarch64-linux-android)
    export CC_aarch64_linux_android="${CC_aarch64_linux_android:-aarch64-linux-android28-clang}"
    export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="${CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER:-$CC_aarch64_linux_android}"
    export AR_aarch64_linux_android="${AR_aarch64_linux_android:-llvm-ar}"
    ;;
  x86_64-linux-android)
    export CC_x86_64_linux_android="${CC_x86_64_linux_android:-x86_64-linux-android28-clang}"
    export CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="${CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER:-$CC_x86_64_linux_android}"
    export AR_x86_64_linux_android="${AR_x86_64_linux_android:-llvm-ar}"
    ;;
  *)
    echo "unsupported --target $TARGET" >&2
    exit 1
    ;;
esac

if ! rustc --print sysroot --target "$TARGET" >/dev/null 2>&1; then
  echo "error: rustc missing $TARGET" >&2
  echo "  rustup target add $TARGET" >&2
  exit 1
fi

ensure_release_signing() {
  local keystore="$HOME/.android/chat-release.keystore"
  if [[ ! -f "$keystore" ]]; then
    echo "generating release keystore at $keystore" >&2
    mkdir -p "$HOME/.android"
    keytool -genkeypair -v \
      -keystore "$keystore" \
      -alias chat \
      -keyalg RSA -keysize 2048 -validity 10000 \
      -storepass android -keypass android \
      -dname "CN=Chat, OU=nandi.uk, O=nandi, L=Unknown, ST=Unknown, C=US" \
      >/dev/null
  fi
  if ! grep -q 'signing.release' "$APP/Cargo.toml"; then
    cat >>"$APP/Cargo.toml" <<EOF

[package.metadata.android.signing.release]
path = "$keystore"
keystore_password = "android"
key_alias = "chat"
key_password = "android"
EOF
    echo "note: appended signing.release to android/Cargo.toml (local only)" >&2
  fi
}

profile=debug
apk_args=(build --target "$TARGET" -p chat-android --lib)
if [[ "$RELEASE" -eq 1 ]]; then
  profile=release
  apk_args+=(--release)
  ensure_release_signing
fi

echo "cargo apk ${apk_args[*]}  (in-tree → $APP)" >&2
echo "rustc $(rustc --version) | $(command -v rustc)" >&2

chmod -R u+w \
  "$APP/target/${profile}/apk" \
  "$APP/target/apk" \
  2>/dev/null || true

# cargo-apk may fail after packaging if its signer wrapper is broken on NixOS;
# the unsigned APK is still written — we sign below when needed.
set +e
(
  cd "$APP"
  cargo apk "${apk_args[@]}" >&2
)
apk_rc=$?
set -e

apk=""
for cand in \
  "$APP/target/${profile}/apk/chat.apk" \
  "$APP/target/${TARGET}/${profile}/apk/chat.apk"; do
  if [[ -f "$cand" ]]; then
    apk="$cand"
    break
  fi
done
if [[ -z "$apk" ]]; then
  apk="$(find "$APP/target" -type f -name 'chat.apk' ! -name '*-unaligned.apk' 2>/dev/null | head -1 || true)"
fi
if [[ -z "${apk:-}" || ! -f "$apk" ]]; then
  echo "APK not found under $APP/target (cargo-apk exit $apk_rc)" >&2
  exit 1
fi

if [[ "$RELEASE" -eq 1 ]]; then
  keystore="$HOME/.android/chat-release.keystore"
  if command -v apksigner >/dev/null; then
    echo "signing $apk (v2+v3)" >&2
    # Prefer v2 (+ v3). Pure v3-only packages confuse some OEM installers.
    # Strip any partial signature from cargo-apk's failed signer first.
    unsigned="$(mktemp --suffix=.apk)"
    python3 - "$apk" "$unsigned" <<'PY'
import sys, zipfile
src, dst = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(src) as zin, zipfile.ZipFile(dst, "w") as zout:
    for info in zin.infolist():
        if info.filename.startswith("META-INF/"):
            continue
        zout.writestr(info, zin.read(info.filename))
PY
    aligned="$(mktemp --suffix=.apk)"
    if command -v zipalign >/dev/null; then
      zipalign -f -p 4 "$unsigned" "$aligned"
    else
      cp -f "$unsigned" "$aligned"
    fi
    apksigner sign \
      --ks "$keystore" \
      --ks-key-alias chat \
      --ks-pass pass:android \
      --key-pass pass:android \
      --v1-signing-enabled true \
      --v2-signing-enabled true \
      --v3-signing-enabled true \
      --out "$apk" \
      "$aligned" >&2
    rm -f "$unsigned" "$aligned"
    apksigner verify -v "$apk" >&2 | head -10 || apksigner verify "$apk" >&2
  elif [[ "$apk_rc" -ne 0 ]]; then
    echo "warning: apksigner missing and cargo-apk exited $apk_rc — APK may be unsigned" >&2
  fi
fi

echo "$apk"
ls -lh "$apk" >&2
