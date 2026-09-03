#!/usr/bin/env bash
# ==============================================================================
#  ⚡ Antigravity /shake & /full-shake Multi-Platform Installer & Uninstaller
# ==============================================================================
set -euo pipefail

REPO="shitan198u/antigravity-shake-skill"
BIN_NAME="shake-prune"
INSTALL_DIR="${HOME}/.gemini/bin"
GLOBAL_SKILLS_DIR="${HOME}/.gemini/config/skills/shake"
FULL_SHAKE_SKILLS_DIR="${HOME}/.gemini/config/skills/full-shake"
HOOKS_CONFIG="${HOME}/.gemini/config/hooks.json"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ==============================================================================
# UNINSTALL MODE
# ==============================================================================
if [ "${1:-}" = "--uninstall" ] || [ "${1:-}" = "-u" ]; then
    echo "⚡ Uninstalling Antigravity /shake & /full-shake..."

    # 1. Remove binary
    if [ -f "${INSTALL_DIR}/${BIN_NAME}" ]; then
        rm -f "${INSTALL_DIR}/${BIN_NAME}"
        echo "  ✓ Removed ${INSTALL_DIR}/${BIN_NAME}"
    fi

    # 2. Remove skills
    if [ -d "${GLOBAL_SKILLS_DIR}" ]; then
        rm -rf "${GLOBAL_SKILLS_DIR}"
        echo "  ✓ Removed ${GLOBAL_SKILLS_DIR}"
    fi
    if [ -d "${FULL_SHAKE_SKILLS_DIR}" ]; then
        rm -rf "${FULL_SHAKE_SKILLS_DIR}"
        echo "  ✓ Removed ${FULL_SHAKE_SKILLS_DIR}"
    fi

    # 3. Clean hook from hooks.json
    if [ -f "${HOOKS_CONFIG}" ]; then
        if command -v jq >/dev/null 2>&1; then
            jq --arg bin "${INSTALL_DIR}/${BIN_NAME} --hook" '
                if .hooks then
                    if .hooks.PreInvocation then
                        .hooks.PreInvocation = (.hooks.PreInvocation | map(select(.command != $bin and (.command | contains("shake-prune") | not))))
                    else . end |
                    if .hooks.Stop then
                        .hooks.Stop = (.hooks.Stop | map(select(.command != $bin and (.command | contains("shake-prune") | not))))
                    else . end
                else . end
            ' "${HOOKS_CONFIG}" > "${HOOKS_CONFIG}.tmp" && mv "${HOOKS_CONFIG}.tmp" "${HOOKS_CONFIG}"
            echo "  ✓ Cleaned PreInvocation and Stop hooks from ${HOOKS_CONFIG}"
        elif command -v python3 >/dev/null 2>&1; then
            python3 -c '
import json, os
config_path = os.path.expanduser("'"${HOOKS_CONFIG}"'")
if os.path.exists(config_path):
    try:
        with open(config_path, "r", encoding="utf-8") as f:
            data = json.load(f)
        if "hooks" in data:
            if "PreInvocation" in data["hooks"]:
                data["hooks"]["PreInvocation"] = [
                    h for h in data["hooks"]["PreInvocation"]
                    if not ("shake-prune" in str(h.get("command", "")))
                ]
            if "Stop" in data["hooks"]:
                data["hooks"]["Stop"] = [
                    h for h in data["hooks"]["Stop"]
                    if not ("shake-prune" in str(h.get("command", "")))
                ]
        with open(config_path, "w", encoding="utf-8") as f:
            json.dump(data, f, indent=2)
        print("  ✓ Cleaned PreInvocation hook via python3")
    except Exception as e:
        print(f"  ⚠️ Could not clean hooks: {e}")
'
        fi
    fi

    echo ""
    echo "🎉 Antigravity /shake has been completely uninstalled."
    exit 0
fi

# ==============================================================================
# INSTALL MODE
# ==============================================================================
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
            x86_64|amd64) TARGET="linux-x86_64" ;;
            aarch64|arm64) TARGET="linux-aarch64" ;;
            *) echo "❌ Unsupported Linux architecture: ${ARCH}"; exit 1 ;;
        esac
        ;;
    darwin)
        TARGET="macos-universal"
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
    # Determine release URL (pinned tag via SHAKE_VERSION; explicit "latest" opts into floating)
    SHAKE_VERSION="${SHAKE_VERSION:-v0.1.10}"
    if [ "${SHAKE_VERSION}" = "latest" ]; then
        BASE_RELEASE_URL="https://github.com/${REPO}/releases/latest/download"
    else
        BASE_RELEASE_URL="https://github.com/${REPO}/releases/download/${SHAKE_VERSION}"
    fi

    DOWNLOAD_URL="${BASE_RELEASE_URL}/${BIN_NAME}-${TARGET}"
    CHECKSUM_URL="${BASE_RELEASE_URL}/SHA256SUMS.txt"
    
    TMP_DIR="$(mktemp -d)"
    trap 'rm -rf "${TMP_DIR}"' EXIT
    
    echo "🌐 Downloading precompiled binary (${BIN_NAME}-${TARGET}) from ${BASE_RELEASE_URL}..."
    if ! curl -fsSL "${DOWNLOAD_URL}" -o "${TMP_DIR}/${BIN_NAME}"; then
        echo "❌ Error: Failed to download precompiled binary from ${DOWNLOAD_URL}" >&2
        exit 1
    fi

    echo "🔒 Downloading and verifying SHA256 integrity checksum..."
    if ! curl -fsSL "${CHECKSUM_URL}" -o "${TMP_DIR}/SHA256SUMS.txt"; then
        echo "❌ Error: Failed to download SHA256SUMS.txt checksums from ${CHECKSUM_URL}. Aborting for supply-chain integrity." >&2
        exit 1
    fi

    EXPECTED_HASH="$(awk -v asset="${BIN_NAME}-${TARGET}" '{gsub(/\r/, "", $2); if($2==asset) print $1}' "${TMP_DIR}/SHA256SUMS.txt")"
    if [ -z "${EXPECTED_HASH}" ]; then
        echo "❌ Error: Asset ${BIN_NAME}-${TARGET} not found in SHA256SUMS.txt. Aborting." >&2
        exit 1
    fi

    if command -v sha256sum >/dev/null 2>&1; then
        ACTUAL_HASH="$(sha256sum "${TMP_DIR}/${BIN_NAME}" | awk '{print $1}')"
    elif command -v shasum >/dev/null 2>&1; then
        ACTUAL_HASH="$(shasum -a 256 "${TMP_DIR}/${BIN_NAME}" | awk '{print $1}')"
    else
        echo "❌ Error: Neither sha256sum nor shasum is available to verify binary integrity." >&2
        exit 1
    fi

    if [ "${EXPECTED_HASH}" != "${ACTUAL_HASH}" ]; then
        echo "❌ Error: SHA256 checksum mismatch for ${BIN_NAME}-${TARGET}!" >&2
        echo "  Expected: ${EXPECTED_HASH}" >&2
        echo "  Actual:   ${ACTUAL_HASH}" >&2
        exit 1
    fi
    echo "  ✓ Checksum successfully verified: ${ACTUAL_HASH}"

    cp "${TMP_DIR}/${BIN_NAME}" "${INSTALL_DIR}/${BIN_NAME}"
    chmod 755 "${INSTALL_DIR}/${BIN_NAME}"
fi

if [ ! -x "${INSTALL_DIR}/${BIN_NAME}" ]; then
    echo "❌ Error: shake-prune binary was not installed successfully to ${INSTALL_DIR}/${BIN_NAME}" >&2
    exit 1
fi

# Install Global Skills
mkdir -p "${GLOBAL_SKILLS_DIR}/references"
mkdir -p "${GLOBAL_SKILLS_DIR}/bin"
mkdir -p "${FULL_SHAKE_SKILLS_DIR}"

# Provide convenient skill-local symlink or copy to ensure legacy relative references resolve
cp "${INSTALL_DIR}/${BIN_NAME}" "${GLOBAL_SKILLS_DIR}/bin/${BIN_NAME}"
chmod 755 "${GLOBAL_SKILLS_DIR}/bin/${BIN_NAME}"

if [ -f "${SCRIPT_DIR}/skills/shake/SKILL.md" ]; then
    cp "${SCRIPT_DIR}/skills/shake/SKILL.md" "${GLOBAL_SKILLS_DIR}/SKILL.md"
elif [ -f "${SCRIPT_DIR}/SKILL.md" ]; then
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

echo "⚙️ Configuring background PreInvocation hook (preserving existing user hooks)..."
if [ -f "${HOOKS_CONFIG}" ]; then
    cp "${HOOKS_CONFIG}" "${HOOKS_CONFIG}.bak" 2>/dev/null || true
    EXISTING_CONTENT="$(cat "${HOOKS_CONFIG}")"
    if [ -z "${EXISTING_CONTENT// }" ]; then
        EXISTING_CONTENT="{}"
    fi
else
    EXISTING_CONTENT="{}"
fi

if command -v jq >/dev/null 2>&1; then
    echo "${EXISTING_CONTENT}" | jq --arg bin "${HOOK_BIN} --hook" '
        del(."shake-anchor") |
        .hooks = (.hooks // {}) |
        .hooks.PreInvocation = (
            ((.hooks.PreInvocation // []) | map(select((.command != $bin) and (.command | contains("shake-prune") | not)))) +
            [{"command": $bin}]
        ) |
        .hooks.Stop = (
            ((.hooks.Stop // []) | map(select((.command != $bin) and (.command | contains("shake-prune") | not)))) +
            [{"command": $bin}]
        )
    ' > "${HOOKS_CONFIG}.tmp"
    mv "${HOOKS_CONFIG}.tmp" "${HOOKS_CONFIG}"
elif command -v python3 >/dev/null 2>&1; then
    python3 -c '
import json, os

config_path = os.path.expanduser("'"${HOOKS_CONFIG}"'")
hook_cmd = "'"${HOOK_BIN}"' --hook"

data = {}
if os.path.exists(config_path):
    try:
        with open(config_path, "r", encoding="utf-8") as f:
            data = json.load(f)
    except Exception:
        data = {}

# Remove legacy shake-anchor property if present
if "shake-anchor" in data:
    del data["shake-anchor"]

if "hooks" not in data or not isinstance(data["hooks"], dict):
    data["hooks"] = {}

for hook_name in ["PreInvocation", "Stop"]:
    hook_list = data["hooks"].get(hook_name, [])
    if not isinstance(hook_list, list):
        hook_list = []
    filtered = [
        h for h in hook_list
        if isinstance(h, dict) and not ("shake-prune" in str(h.get("command", "")))
    ]
    filtered.append({"command": hook_cmd})
    data["hooks"][hook_name] = filtered

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
echo "• Proactive 80k token auto-compaction hook configured in: ${HOOKS_CONFIG}"
echo ""
echo "👉 Type /shake or /full-shake in any Antigravity conversation to compact context!"
