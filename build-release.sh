#!/usr/bin/env bash
# Multiplatform Release Builder for Redragon M719-PRO Suite (Linux & macOS)
# Produces standardized release artifacts in ./dist/
# Naming pattern: redragon_m719_pro-<os>-<arch>

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DIST_DIR="${SCRIPT_DIR}/dist"
mkdir -p "${DIST_DIR}"

build_target() {
    local target="$1"
    local out_name="$2"

    echo ""
    echo "========================================================"
    echo " Building Target: ${target} -> dist/${out_name}"
    echo "========================================================"

    rustup target add "${target}"
    cargo build --release --bin redragon_m719_pro --target "${target}"

    cp "${SCRIPT_DIR}/target/${target}/release/redragon_m719_pro" "${DIST_DIR}/${out_name}"
    chmod +x "${DIST_DIR}/${out_name}"
    echo "[OK] Artifact created: ${DIST_DIR}/${out_name}"
}

OS_NAME="$(uname -s)"

case "${OS_NAME}" in
    Linux*)
        build_target "x86_64-unknown-linux-gnu" "redragon_m719_pro-linux-x64"
        if dpkg --print-foreign-architectures 2>/dev/null | grep -q "i386"; then
            PKG_CONFIG_ALLOW_CROSS=1 \
            PKG_CONFIG_PATH="/usr/lib/i386-linux-gnu/pkgconfig" \
            build_target "i686-unknown-linux-gnu" "redragon_m719_pro-linux-x86"
        else
            echo "[INFO] Skipping i686-unknown-linux-gnu (enable i386 multilib packages to build 32-bit Linux locally)."
        fi
        if [ -d "${HOME}/osxcross/target/bin" ]; then
            export PATH="${HOME}/osxcross/target/bin:${PATH}"
            DARWIN_CC_X64="$(ls "${HOME}"/osxcross/target/bin/x86_64-apple-darwin*-clang | head -n 1)"
            DARWIN_AR_X64="$(ls "${HOME}"/osxcross/target/bin/x86_64-apple-darwin*-ar | head -n 1)"
            DARWIN_CC_ARM64="$(ls "${HOME}"/osxcross/target/bin/aarch64-apple-darwin*-clang | head -n 1)"
            DARWIN_AR_ARM64="$(ls "${HOME}"/osxcross/target/bin/aarch64-apple-darwin*-ar | head -n 1)"

            CC_x86_64_apple_darwin="${DARWIN_CC_X64}" \
            AR_x86_64_apple_darwin="${DARWIN_AR_X64}" \
            CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER="${DARWIN_CC_X64}" \
            build_target "x86_64-apple-darwin" "redragon_m719_pro-macos-x64"

            CC_aarch64_apple_darwin="${DARWIN_CC_ARM64}" \
            AR_aarch64_apple_darwin="${DARWIN_AR_ARM64}" \
            CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER="${DARWIN_CC_ARM64}" \
            build_target "aarch64-apple-darwin" "redragon_m719_pro-macos-arm64"
        fi
        ;;
    Darwin*)
        build_target "x86_64-apple-darwin" "redragon_m719_pro-macos-x64"
        build_target "aarch64-apple-darwin" "redragon_m719_pro-macos-arm64"
        ;;
    *)
        echo "Unsupported OS for build-release.sh: ${OS_NAME}. Use build-release.ps1 on Windows."
        exit 1
        ;;
esac

echo ""
echo "All release artifacts in ${DIST_DIR}:"
ls -lh "${DIST_DIR}"
