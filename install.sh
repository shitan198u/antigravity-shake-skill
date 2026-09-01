#!/usr/bin/env bash
# ==============================================================================
#  ⚡ Antigravity /shake & /full-shake Multi-Platform Installer
# ==============================================================================
set -euo pipefail

REPO="shitan198u/antigravity-shake-skill"
BIN_NAME="shake-prune"
INSTALL_DIR="${HOME}/.gemini/bin"
GLOBAL_SKILLS_DIR="${HOME}/.gemini/config/skills/shake"
FULL_SHAKE_SKILLS_DIR="${HOME}/.gemini/config/skills/full-shake"
HOOKS_CONFIG="${HOME}/.gemini/config/hooks.json"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "⚡ Installing Antigravity /shake & /full-shake Context Compactor..."

# Ensure secure installation directory with 0700 permissions
mkdir -p "${INSTALL_DIR}"
chmod 700 "${INSTALL_DIR}"

# Determine architecture & OS
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "${OS}" in
    linux)
        case "${ARCH}" in
            x86_64|amd64) TARGET="x86_64-unknown-linux-gnu" ;;
            aarch64|arm64) TARGET="aarch64-unknown-linux-gnu" ;;
            *) echo "❌ Unsupported Linux architecture: ${ARCH}"; exit 1 ;;
        esac
        ;;
    darwin)
        TARGET="universal-apple-darwin"
        ;;
    *)
        echo "❌ Unsupported OS: ${OS}. On Windows, run install.ps1 via PowerShell."
        exit 1
        ;;
esac

# Check for local pre-built binary first
if [ -f "${SCRIPT_DIR}/bin/${BIN_NAME}" ]; then
    echo "📦 Using local pre-built binary..."
    cp "${SCRIPT_DIR}/bin/${BIN_NAME}" "${INSTALL_DIR}/${BIN_NAME}"
    chmod 755 "${INSTALL_DIR}/${BIN_NAME}"
elif [ -f "${SCRIPT_DIR}/shake-prune-rs/target/release/${BIN_NAME}" ]; then
    echo "📦 Using local cargo release binary..."
    cp "${SCRIPT_DIR}/shake-prune-rs/target/release/${BIN_NAME}" "${INSTALL_DIR}/${BIN_NAME}"
    chmod 755 "${INSTALL_DIR}/${BIN_NAME}"
elif command -v cargo >/dev/null 2>&1 && [ -f "${SCRIPT_DIR}/shake-prune-rs/Cargo.toml" ]; then
    echo "⚙️ Building from local source via Cargo..."
    cargo build --release --manifest-path "${SCRIPT_DIR}/shake-prune-rs/Cargo.toml"
    cp "${SCRIPT_DIR}/shake-prune-rs/target/release/${BIN_NAME}" "${INSTALL_DIR}/${BIN_NAME}"
    chmod 755 "${INSTALL_DIR}/${BIN_NAME}"
else
    # Download latest release binary from GitHub
    echo "🌐 Fetching release asset for ${TARGET}..."
    DOWNLOAD_URL="https://github.com/${REPO}/releases/latest/download/${BIN_NAME}-${TARGET}"
    CHECKSUM_URL="https://github.com/${REPO}/releases/latest/download/SHA256SUMS"
    
    TMP_DIR="$(mktemp -d)"
    trap 'rm -rf "${TMP_DIR}"' EXIT
    
    curl -fsSL "${DOWNLOAD_URL}" -o "${TMP_DIR}/${BIN_NAME}"
    curl -fsSL "${CHECKSUM_URL}" -o "${TMP_DIR}/SHA256SUMS" 2>/dev/null || true
    
    # Verify SHA256 checksum if available
    if [ -f "${TMP_DIR}/SHA256SUMS" ]; then
        echo "🔒 Verifying SHA256 integrity..."
        cd "${TMP_DIR}"
        EXPECTED_HASH="$(awk -v asset="${BIN_NAME}-${TARGET}" '{gsub(/\r/, "", $2); if($2==asset) print $1}' SHA256SUMS)"
        if [ -n "${EXPECTED_HASH}" ]; then
            ACTUAL_HASH="$(sha256sum "${BIN_NAME}" | awk '{print $1}')"
            if [ "${EXPECTED_HASH}" != "${ACTUAL_HASH}" ]; then
                echo "❌ SHA256 verification failed!"
                exit 1
            fi
            echo "  ✓ Checksum verified!"
        fi
        cd "${SCRIPT_DIR}"
    fi
    
    cp "${TMP_DIR}/${BIN_NAME}" "${INSTALL_DIR}/${BIN_NAME}"
    chmod 755 "${INSTALL_DIR}/${BIN_NAME}"
fi

# Install Global Skills
mkdir -p "${GLOBAL_SKILLS_DIR}/references"
mkdir -p "${FULL_SHAKE_SKILLS_DIR}"

if [ -f "${SCRIPT_DIR}/SKILL.md" ]; then
    cp "${SCRIPT_DIR}/SKILL.md" "${GLOBAL_SKILLS_DIR}/SKILL.md"
fi
if [ -d "${SCRIPT_DIR}/references" ]; then
    cp -r "${SCRIPT_DIR}/references/"* "${GLOBAL_SKILLS_DIR}/references/" 2>/dev/null || true
fi
if [ -f "${SCRIPT_DIR}/skills/full-shake/SKILL.md" ]; then
    cp "${SCRIPT_DIR}/skills/full-shake/SKILL.md" "${FULL_SHAKE_SKILLS_DIR}/SKILL.md"
fi

# Configure Background PreInvocation Hook in ~/.gemini/config/hooks.json
HOOK_BIN="${INSTALL_DIR}/${BIN_NAME}"
mkdir -p "$(dirname "${HOOKS_CONFIG}")"

echo "⚙️ Configuring background PreInvocation hook..."
if command -v jq >/dev/null 2>&1; then
    if [ -f "${HOOKS_CONFIG}" ]; then
        EXISTING_CONTENT="$(cat "${HOOKS_CONFIG}")"
        if [ -z "${EXISTING_CONTENT// }" ]; then
            EXISTING_CONTENT="{}"
        fi
    else
        EXISTING_CONTENT="{}"
    fi

    echo "${EXISTING_CONTENT}" | jq --arg bin "${HOOK_BIN} --hook" '
        .hooks = (.hooks // {}) |
        .hooks.PreInvocation = (
            ((.hooks.PreInvocation // []) | map(select(.command != $bin))) +
            [{"command": $bin}]
        )
    ' > "${HOOKS_CONFIG}.tmp"
    mv "${HOOKS_CONFIG}.tmp" "${HOOKS_CONFIG}"
elif command -v python3 >/dev/null 2>&1; then
    # Safe Python fallback for JSON manipulation
    python3 -c '
import json, os, sys

config_path = os.path.expanduser("'"${HOOKS_CONFIG}"'")
hook_cmd = "'"${HOOK_BIN}"' --hook"

data = {}
if os.path.exists(config_path):
    try:
        with open(config_path, "r", encoding="utf-8") as f:
            data = json.load(f)
    except Exception:
        data = {}

if "hooks" not in data or not isinstance(data["hooks"], dict):
    data["hooks"] = {}

pre_inv = data["hooks"].get("PreInvocation", [])
if not isinstance(pre_inv, list):
    pre_inv = []

filtered = [h for h in pre_inv if isinstance(h, dict) and h.get("command") != hook_cmd]
filtered.append({"command": hook_cmd})
data["hooks"]["PreInvocation"] = filtered

with open(config_path, "w", encoding="utf-8") as f:
    json.dump(data, f, indent=2)
'
else
    echo "⚠️ Warning: Neither jq nor python3 was found. Please ensure ${HOOKS_CONFIG} contains PreInvocation hook."
fi

echo ""
echo "🎉 Installation Complete!"
echo "• Binary installed to: ${INSTALL_DIR}/${BIN_NAME}"
echo "• /shake skill installed to: ${GLOBAL_SKILLS_DIR}"
echo "• /full-shake skill installed to: ${FULL_SHAKE_SKILLS_DIR}"
echo "• Proactive 200k token auto-compaction hook configured in: ${HOOKS_CONFIG}"
echo ""
echo "👉 Type /shake or /full-shake in any Antigravity conversation to compact context!"
