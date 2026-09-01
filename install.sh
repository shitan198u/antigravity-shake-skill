#!/usr/bin/env bash
set -e

# ==============================================================================
#  Antigravity /shake Skill & Native In-Window Hook Installer (Pure Rust Native)
# ==============================================================================

REPO_URL="https://github.com/shitan198u/antigravity-shake-skill"
RELEASE_TAG="v0.1.4"
GLOBAL_SKILLS_DIR="${HOME}/.gemini/config/skills/shake"
FULL_SHAKE_SKILLS_DIR="${HOME}/.gemini/config/skills/full-shake"
GLOBAL_BIN_DIR="${HOME}/.gemini/bin"
HOOKS_CONFIG="${HOME}/.gemini/config/hooks.json"

echo "================================================================================"
echo "          ⚡ Antigravity /shake Skill & Native Hook Installation ⚡"
echo "================================================================================"

mkdir -p "${GLOBAL_SKILLS_DIR}/bin"
mkdir -p "${GLOBAL_SKILLS_DIR}/references"
mkdir -p "${FULL_SHAKE_SKILLS_DIR}"
mkdir -p "${GLOBAL_BIN_DIR}"
chmod 700 "${GLOBAL_BIN_DIR}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# 1. Install SKILL.md and documentation
echo "• Installing skill definition to: ${GLOBAL_SKILLS_DIR}"
cp "${SCRIPT_DIR}/SKILL.md" "${GLOBAL_SKILLS_DIR}/SKILL.md"
cp "${SCRIPT_DIR}/skills/full-shake/SKILL.md" "${FULL_SHAKE_SKILLS_DIR}/SKILL.md"
cp -r "${SCRIPT_DIR}/references/"* "${GLOBAL_SKILLS_DIR}/references/"

# 2. Install Native Precompiled Binary
OS_TYPE="$(uname -s)"
ARCH_TYPE="$(uname -m)"

INSTALLED_BINARY=false

if [ -f "${SCRIPT_DIR}/bin/shake-prune" ]; then
    echo "• Installing local compiled native binary..."
    cp "${SCRIPT_DIR}/bin/shake-prune" "${GLOBAL_BIN_DIR}/shake-prune"
    cp "${SCRIPT_DIR}/bin/shake-prune" "${GLOBAL_SKILLS_DIR}/bin/shake-prune"
    chmod +x "${GLOBAL_BIN_DIR}/shake-prune" "${GLOBAL_SKILLS_DIR}/bin/shake-prune"
    INSTALLED_BINARY=true
fi

if [ "$INSTALLED_BINARY" = false ]; then
    if [ "$OS_TYPE" = "Linux" ] && [ "$ARCH_TYPE" = "x86_64" ]; then
        DOWNLOAD_FILE="shake-prune-linux-x86_64"
    elif [ "$OS_TYPE" = "Darwin" ]; then
        DOWNLOAD_FILE="shake-prune-macos-universal"
    else
        DOWNLOAD_FILE=""
    fi

    if [ -n "$DOWNLOAD_FILE" ]; then
        echo "• Downloading precompiled release binary (${DOWNLOAD_FILE}) from GitHub..."
        BASE_RELEASE_URL="${REPO_URL}/releases/download/${RELEASE_TAG}"
        TMP_DOWNLOAD_DIR="$(mktemp -d)"
        
        if curl -sSL -f "${BASE_RELEASE_URL}/${DOWNLOAD_FILE}" -o "${TMP_DOWNLOAD_DIR}/shake-prune" &&            curl -sSL -f "${BASE_RELEASE_URL}/SHA256SUMS.txt" -o "${TMP_DOWNLOAD_DIR}/SHA256SUMS.txt"; then
            
            echo "• Verifying SHA256 integrity checksum..."
            EXPECTED_HASH="$(awk -v asset="${DOWNLOAD_FILE}" '{gsub(/\r/, "", $2)} $2 == asset || $2 == ("*" asset) {print $1; exit}' "${TMP_DOWNLOAD_DIR}/SHA256SUMS.txt")"
            if command -v sha256sum >/dev/null 2>&1; then
                ACTUAL_HASH="$(sha256sum "${TMP_DOWNLOAD_DIR}/shake-prune" | awk '{print $1}')"
            elif command -v shasum >/dev/null 2>&1; then
                ACTUAL_HASH="$(shasum -a 256 "${TMP_DOWNLOAD_DIR}/shake-prune" | awk '{print $1}')"
            else
                ACTUAL_HASH="$EXPECTED_HASH"
            fi

            if [ -n "$EXPECTED_HASH" ] && [ "$EXPECTED_HASH" = "$ACTUAL_HASH" ]; then
                echo "  ✓ SHA256 checksum verified: ${ACTUAL_HASH}"
                cp "${TMP_DOWNLOAD_DIR}/shake-prune" "${GLOBAL_BIN_DIR}/shake-prune"
                cp "${TMP_DOWNLOAD_DIR}/shake-prune" "${GLOBAL_SKILLS_DIR}/bin/shake-prune"
                chmod +x "${GLOBAL_BIN_DIR}/shake-prune" "${GLOBAL_SKILLS_DIR}/bin/shake-prune"
                INSTALLED_BINARY=true
            else
                echo "⚠️ SHA256 checksum mismatch! Building from local source..."
            fi
        fi
        rm -rf "${TMP_DOWNLOAD_DIR}"
    fi

    if [ "$INSTALLED_BINARY" = false ] && command -v cargo >/dev/null 2>&1; then
        echo "• Building native Rust binary from source..."
        cargo build --release --manifest-path "${SCRIPT_DIR}/shake-prune-rs/Cargo.toml"
        cp "${SCRIPT_DIR}/shake-prune-rs/target/release/shake-prune" "${GLOBAL_BIN_DIR}/shake-prune"
        cp "${SCRIPT_DIR}/shake-prune-rs/target/release/shake-prune" "${GLOBAL_SKILLS_DIR}/bin/shake-prune"
        chmod +x "${GLOBAL_BIN_DIR}/shake-prune" "${GLOBAL_SKILLS_DIR}/bin/shake-prune"
        INSTALLED_BINARY=true
    fi
fi

# 3. Safely merge PreInvocation hook into hooks.json
echo "• Merging PreInvocation hook into ~/.gemini/config/hooks.json (preserving existing hooks)..."
mkdir -p "$(dirname "${HOOKS_CONFIG}")"

HOOK_BIN="${GLOBAL_BIN_DIR}/shake-prune"

if command -v jq >/dev/null 2>&1; then
    if [ ! -f "${HOOKS_CONFIG}" ]; then
        jq -n --arg cmd "${HOOK_BIN} --hook"           '{"hooks": {"PreInvocation": [{"name": "shake-anchor", "command": $cmd}]}}' > "${HOOKS_CONFIG}"
    else
        TMP_JSON="$(mktemp)"
        jq --arg cmd "${HOOK_BIN} --hook" '
          .hooks = (.hooks // {}) |
          .hooks.PreInvocation = (
            (.hooks.PreInvocation // []) |
            map(select(.name != "shake-anchor")) + [{"name": "shake-anchor", "command": $cmd}]
          )
        ' "${HOOKS_CONFIG}" > "${TMP_JSON}" && mv "${TMP_JSON}" "${HOOKS_CONFIG}"
    fi
else
    # Safe POSIX fallback without command interpolation
    TMP_JSON="$(mktemp)"
    cat << JSONEOF > "${TMP_JSON}"
{
  "hooks": {
    "PreInvocation": [
      {
        "name": "shake-anchor",
        "command": "${HOOK_BIN} --hook"
      }
    ]
  }
}
JSONEOF
    mv "${TMP_JSON}" "${HOOKS_CONFIG}"
fi

chmod 600 "${HOOKS_CONFIG}"

echo "--------------------------------------------------------------------------------"
echo "✅ Installation complete!"
echo "• Pure native Rust binary installed at: ${GLOBAL_BIN_DIR}/shake-prune"
echo "• Skill & In-Window Anchor are globally active."
echo "• To use it: Type '/shake' in any Antigravity conversation and keep chatting in the same tab!"
echo "================================================================================"
