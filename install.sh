#!/usr/bin/env bash
# ==============================================================================
# Antigravity `/shake` Skill Installer (Linux & macOS)
# Installs the high-speed /shake context-pruning skill globally for Antigravity,
# with native PreInvocation hook support (Zero Python required).
# ==============================================================================

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TARGET_CONFIG_DIR="${HOME}/.gemini/config"
TARGET_SKILL_DIR="${TARGET_CONFIG_DIR}/skills/shake"
TARGET_BIN_DIR="${HOME}/.gemini/bin"
REPO_URL="https://github.com/shitan198u/antigravity-shake-skill"

echo "================================================================================"
echo "          ⚡ Antigravity /shake Skill & Native Hook Installation ⚡"
echo "================================================================================"

# 1. Ensure target directories exist
mkdir -p "${TARGET_SKILL_DIR}/scripts"
mkdir -p "${TARGET_SKILL_DIR}/references"
mkdir -p "${TARGET_SKILL_DIR}/assets"
mkdir -p "${TARGET_SKILL_DIR}/bin"
mkdir -p "${TARGET_BIN_DIR}"

# 2. Copy Skill definition, fallback scripts, assets, and reference documentation
echo "• Installing skill definition to: ${TARGET_SKILL_DIR}"
cp "${SCRIPT_DIR}/SKILL.md" "${TARGET_SKILL_DIR}/SKILL.md"
cp "${SCRIPT_DIR}/scripts/shake_prune.py" "${TARGET_SKILL_DIR}/scripts/shake_prune.py"
chmod +x "${TARGET_SKILL_DIR}/scripts/shake_prune.py"
cp "${SCRIPT_DIR}/references/omp_comparison.md" "${TARGET_SKILL_DIR}/references/omp_comparison.md"

if [ -f "${SCRIPT_DIR}/assets/artifact_preview.png" ]; then
    cp "${SCRIPT_DIR}/assets/artifact_preview.png" "${TARGET_SKILL_DIR}/assets/artifact_preview.png"
fi

# 3. Binary Installation (Prebuilt -> GitHub Release Download -> Cargo Compile -> Python Fallback)
PREBUILT_BIN="${SCRIPT_DIR}/bin/shake-prune"
BINARY_INSTALLED=false

# Check if local prebuilt binary executes
if [ -f "${PREBUILT_BIN}" ] && "${PREBUILT_BIN}" --help >/dev/null 2>&1; then
    echo "• Installing precompiled native binary to: ${TARGET_BIN_DIR}/shake-prune"
    cp "${PREBUILT_BIN}" "${TARGET_BIN_DIR}/shake-prune"
    chmod +x "${TARGET_BIN_DIR}/shake-prune"
    cp "${PREBUILT_BIN}" "${TARGET_SKILL_DIR}/bin/shake-prune"
    chmod +x "${TARGET_SKILL_DIR}/bin/shake-prune"
    BINARY_INSTALLED=true
fi

# If on macOS and local binary failed (likely Linux binary), try downloading macOS universal binary from GitHub Releases
OS_NAME="$(uname -s)"
if [ "${BINARY_INSTALLED}" = false ] && [ "${OS_NAME}" = "Darwin" ]; then
    echo "• macOS detected. Fetching universal precompiled release binary from GitHub..."
    MAC_BIN_URL="${REPO_URL}/releases/latest/download/shake-prune-macos-universal"
    if curl -sLf "${MAC_BIN_URL}" -o "${TARGET_BIN_DIR}/shake-prune" 2>/dev/null && chmod +x "${TARGET_BIN_DIR}/shake-prune" && "${TARGET_BIN_DIR}/shake-prune" --help >/dev/null 2>&1; then
        cp "${TARGET_BIN_DIR}/shake-prune" "${TARGET_SKILL_DIR}/bin/shake-prune"
        echo "• Installed macOS universal binary from release to: ${TARGET_BIN_DIR}/shake-prune"
        BINARY_INSTALLED=true
    fi
fi

# If still not installed, try compiling from source via cargo
if [ "${BINARY_INSTALLED}" = false ] && command -v cargo >/dev/null 2>&1 && [ -f "${SCRIPT_DIR}/shake-prune-rs/Cargo.toml" ]; then
    echo "• Compiling native binary from source via cargo..."
    cargo build --release --manifest-path "${SCRIPT_DIR}/shake-prune-rs/Cargo.toml"
    RUST_BIN="${SCRIPT_DIR}/shake-prune-rs/target/release/shake-prune"
    if [ -f "${RUST_BIN}" ]; then
        cp "${RUST_BIN}" "${TARGET_BIN_DIR}/shake-prune"
        chmod +x "${TARGET_BIN_DIR}/shake-prune"
        cp "${RUST_BIN}" "${TARGET_SKILL_DIR}/bin/shake-prune"
        chmod +x "${TARGET_SKILL_DIR}/bin/shake-prune"
        echo "• Installed compiled native binary to: ${TARGET_BIN_DIR}/shake-prune"
        BINARY_INSTALLED=true
    fi
fi

if [ "${BINARY_INSTALLED}" = false ]; then
    echo "• Note: Using universal Python fallback engine (scripts/shake_prune.py)."
fi

# 4. Install Native PreInvocation Hook in ~/.gemini/config/hooks.json (Pure Bash)
echo "• Registering native PreInvocation lifecycle hook in ~/.gemini/config/hooks.json..."
cat << 'HOOK_EOF' > "${TARGET_CONFIG_DIR}/hooks.json"
{
  "shake-anchor": {
    "enabled": true,
    "PreInvocation": [
      {
        "type": "command",
        "command": "~/.gemini/bin/shake-prune --hook"
      }
    ]
  }
}
HOOK_EOF

echo "--------------------------------------------------------------------------------"
echo "✅ Installation complete!"
echo "• Skill & Native In-Window Anchor are globally active."
echo "• To use it: Type '/shake' in any Antigravity conversation and keep chatting in the same tab!"
echo "================================================================================"
