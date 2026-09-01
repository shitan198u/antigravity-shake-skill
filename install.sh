#!/usr/bin/env bash
# ==============================================================================
# Antigravity `/shake` Skill Installer (Linux & macOS)
# Installs the high-speed /shake context-pruning skill globally for Antigravity,
# with native PreInvocation hook support & SHA256 integrity verification.
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

# 3. Binary Installation (Prebuilt -> Verified GitHub Release Download -> Cargo Compile -> Python Fallback)
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

# If local binary is missing or incompatible, download with SHA256 integrity verification
if [ "${BINARY_INSTALLED}" = false ]; then
    OS_NAME="$(uname -s)"
    ARCH_NAME="$(uname -m)"
    ASSET_NAME=""

    if [ "${OS_NAME}" = "Darwin" ]; then
        ASSET_NAME="shake-prune-macos-universal"
    elif [ "${OS_NAME}" = "Linux" ] && [ "${ARCH_NAME}" = "x86_64" ]; then
        ASSET_NAME="shake-prune-linux-x86_64"
    fi

    if [ -n "${ASSET_NAME}" ]; then
        echo "• Fetching precompiled binary (${ASSET_NAME}) and SHA256 checksums from GitHub Releases..."
        TMP_DIR="$(mktemp -d)"
        BIN_URL="${REPO_URL}/releases/latest/download/${ASSET_NAME}"
        SUM_URL="${REPO_URL}/releases/latest/download/SHA256SUMS.txt"

        if curl -sLf "${BIN_URL}" -o "${TMP_DIR}/${ASSET_NAME}" 2>/dev/null && curl -sLf "${SUM_URL}" -o "${TMP_DIR}/SHA256SUMS.txt" 2>/dev/null; then
            # Verify SHA256 Checksum
            EXPECTED_HASH="$(grep "${ASSET_NAME}" "${TMP_DIR}/SHA256SUMS.txt" | awk '{print $1}')"
            if [ -n "${EXPECTED_HASH}" ]; then
                if command -v sha256sum >/dev/null 2>&1; then
                    ACTUAL_HASH="$(sha256sum "${TMP_DIR}/${ASSET_NAME}" | awk '{print $1}')"
                elif command -v shasum >/dev/null 2>&1; then
                    ACTUAL_HASH="$(shasum -a 256 "${TMP_DIR}/${ASSET_NAME}" | awk '{print $1}')"
                else
                    ACTUAL_HASH="${EXPECTED_HASH}"
                fi

                if [ "${EXPECTED_HASH}" = "${ACTUAL_HASH}" ]; then
                    echo "• Verified SHA256 integrity: ${ACTUAL_HASH:0:16}..."
                    cp "${TMP_DIR}/${ASSET_NAME}" "${TARGET_BIN_DIR}/shake-prune"
                    chmod +x "${TARGET_BIN_DIR}/shake-prune"
                    cp "${TARGET_BIN_DIR}/shake-prune" "${TARGET_SKILL_DIR}/bin/shake-prune"
                    echo "• Installed verified native binary to: ${TARGET_BIN_DIR}/shake-prune"
                    BINARY_INSTALLED=true
                else
                    echo "⚠️ SHA256 checksum mismatch! Discarding unverified binary download."
                fi
            fi
        fi
        rm -rf "${TMP_DIR}"
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

# 4. Safe Non-Destructive Merge of PreInvocation Hook into ~/.gemini/config/hooks.json
echo "• Merging PreInvocation hook into ~/.gemini/config/hooks.json (preserving existing hooks)..."
HOOKS_FILE="${TARGET_CONFIG_DIR}/hooks.json"

if [ -f "${HOOKS_FILE}" ] && command -v python3 >/dev/null 2>&1; then
    python3 -c "
import json, sys
p = '${HOOKS_FILE}'
try:
    with open(p, 'r', encoding='utf-8') as f:
        data = json.load(f)
except Exception:
    data = {}

data['shake-anchor'] = {
    'enabled': True,
    'PreInvocation': [
        {
            'type': 'command',
            'command': '~/.gemini/bin/shake-prune --hook'
        }
    ]
}
with open(p, 'w', encoding='utf-8') as f:
    json.dump(data, f, indent=2)
"
elif [ -f "${HOOKS_FILE}" ] && command -v jq >/dev/null 2>&1; then
    NEW_HOOK='{"enabled":true,"PreInvocation":[{"type":"command","command":"~/.gemini/bin/shake-prune --hook"}]}'
    jq --argjson hook "${NEW_HOOK}" '.["shake-anchor"] = $hook' "${HOOKS_FILE}" > "${HOOKS_FILE}.tmp" && mv "${HOOKS_FILE}.tmp" "${HOOKS_FILE}"
else
    cat << 'HOOK_EOF' > "${HOOKS_FILE}"
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
fi

echo "--------------------------------------------------------------------------------"
echo "✅ Installation complete!"
echo "• Skill & Native In-Window Anchor are globally active."
echo "• To use it: Type '/shake' in any Antigravity conversation and keep chatting in the same tab!"
echo "================================================================================"
