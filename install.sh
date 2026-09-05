#!/usr/bin/env bash
# ==============================================================================
#  ⚡ Antigravity /shake Context Compactor & Utility Suite Installer
# ==============================================================================
set -euo pipefail

REPO="shitan198u/antigravity-shake-skill"
BIN_NAME="shake-prune"
INSTALL_DIR="${HOME}/.gemini/bin"
GLOBAL_SKILLS_DIR="${HOME}/.gemini/config/skills/shake"
FULL_SHAKE_SKILLS_DIR="${HOME}/.gemini/config/skills/full-shake"
HOOKS_CONFIG="${HOME}/.gemini/config/hooks.json"
SHAKE_VERSION="${SHAKE_VERSION:-v0.2.0}"

# Piped execution safe SCRIPT_DIR detection
SCRIPT_DIR=""
if [ -n "${BASH_SOURCE[0]:-}" ] && [ -f "${BASH_SOURCE[0]}" ]; then
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fi

# Determine if running in local development mode
LOCAL_DEV=0
if [ "${SHAKE_LOCAL_DEV:-0}" = "1" ] || [ "${1:-}" = "--local" ] || [ "${1:-}" = "-l" ]; then
    LOCAL_DEV=1
fi

# ==============================================================================
# UNINSTALL MODE
# ==============================================================================
if [ "${1:-}" = "--uninstall" ] || [ "${1:-}" = "-u" ]; then
    echo "⚡ Uninstalling Antigravity /shake..."

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
    echo "🎉 Antigravity /shake binaries, skills, and hooks removed."
    echo "   Retained (delete manually if desired): ~/.gemini/config/shake.toml, logs, transcript_full.jsonl archives, and .bak files."
    exit 0
fi

# ==============================================================================
# INSTALL MODE
# ==============================================================================
echo "⚡ Installing Antigravity /shake Context Compactor..."

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

REF="${SHAKE_VERSION}"
[ "${REF}" = "latest" ] && REF="main"
RAW_BASE_URL="https://raw.githubusercontent.com/${REPO}/${REF}"

# 1. Binary acquisition
INSTALLED_BIN=0
if [ "${LOCAL_DEV}" = "1" ] && [ -n "${SCRIPT_DIR}" ]; then
    if [ -f "${SCRIPT_DIR}/bin/${BIN_NAME}" ]; then
        echo "📦 [Local Dev] Using local pre-built binary: ${SCRIPT_DIR}/bin/${BIN_NAME}"
        cp -f "${SCRIPT_DIR}/bin/${BIN_NAME}" "${INSTALL_DIR}/${BIN_NAME}"
        chmod 755 "${INSTALL_DIR}/${BIN_NAME}"
        INSTALLED_BIN=1
    elif [ -f "${SCRIPT_DIR}/shake-prune-rs/target/release/${BIN_NAME}" ]; then
        echo "📦 [Local Dev] Using local cargo release binary: ${SCRIPT_DIR}/shake-prune-rs/target/release/${BIN_NAME}"
        cp -f "${SCRIPT_DIR}/shake-prune-rs/target/release/${BIN_NAME}" "${INSTALL_DIR}/${BIN_NAME}"
        chmod 755 "${INSTALL_DIR}/${BIN_NAME}"
        INSTALLED_BIN=1
    fi
fi

if [ "${INSTALLED_BIN}" = "0" ]; then
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
    if ! curl --connect-timeout 15 -fsSL "${DOWNLOAD_URL}" -o "${TMP_DIR}/${BIN_NAME}"; then
        echo "❌ Error: Failed to download precompiled binary from ${DOWNLOAD_URL}" >&2
        echo "   If you have Rust installed, you can build from source: cargo build --release --manifest-path shake-prune-rs/Cargo.toml" >&2
        exit 1
    fi

    echo "🔒 Downloading and verifying SHA256 integrity checksum..."
    if ! curl --connect-timeout 15 -fsSL "${CHECKSUM_URL}" -o "${TMP_DIR}/SHA256SUMS.txt"; then
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

    cp -f "${TMP_DIR}/${BIN_NAME}" "${INSTALL_DIR}/${BIN_NAME}"
    chmod 755 "${INSTALL_DIR}/${BIN_NAME}"
fi

if [ ! -x "${INSTALL_DIR}/${BIN_NAME}" ]; then
    echo "❌ Error: shake-prune binary was not installed successfully to ${INSTALL_DIR}/${BIN_NAME}" >&2
    exit 1
fi

# 2. Skill & Documentation deployment (Always fresh overwrite)
mkdir -p "${GLOBAL_SKILLS_DIR}/references"
mkdir -p "${GLOBAL_SKILLS_DIR}/bin"

cp -f "${INSTALL_DIR}/${BIN_NAME}" "${GLOBAL_SKILLS_DIR}/bin/${BIN_NAME}"
chmod 755 "${GLOBAL_SKILLS_DIR}/bin/${BIN_NAME}"

SKILL_COPIED=0
if [ "${LOCAL_DEV}" = "1" ] && [ -n "${SCRIPT_DIR}" ]; then
    if [ -f "${SCRIPT_DIR}/skills/shake/SKILL.md" ]; then
        echo "📋 [Local Dev] Deploying SKILL.md from local repository..."
        cp -f "${SCRIPT_DIR}/skills/shake/SKILL.md" "${GLOBAL_SKILLS_DIR}/SKILL.md"
        SKILL_COPIED=1
    elif [ -f "${SCRIPT_DIR}/SKILL.md" ]; then
        echo "📋 [Local Dev] Deploying SKILL.md from local repository..."
        cp -f "${SCRIPT_DIR}/SKILL.md" "${GLOBAL_SKILLS_DIR}/SKILL.md"
        SKILL_COPIED=1
    fi
    if [ -d "${SCRIPT_DIR}/references" ]; then
        echo "📚 [Local Dev] Deploying reference documentation from local repository..."
        cp -rf "${SCRIPT_DIR}/references/"* "${GLOBAL_SKILLS_DIR}/references/" 2>/dev/null || true
    fi
fi

if [ "${SKILL_COPIED}" = "0" ]; then
    echo "📋 Downloading latest SKILL.md definition from GitHub (${REF})..."
    if command -v curl >/dev/null 2>&1; then
        curl --connect-timeout 10 -fsSL "${RAW_BASE_URL}/skills/shake/SKILL.md" -o "${GLOBAL_SKILLS_DIR}/SKILL.md" 2>/dev/null || \
        curl --connect-timeout 10 -fsSL "${RAW_BASE_URL}/SKILL.md" -o "${GLOBAL_SKILLS_DIR}/SKILL.md" 2>/dev/null || \
        echo "⚠️ Warning: Could not fetch SKILL.md from ${RAW_BASE_URL}" >&2
    elif command -v wget >/dev/null 2>&1; then
        wget -qO "${GLOBAL_SKILLS_DIR}/SKILL.md" "${RAW_BASE_URL}/skills/shake/SKILL.md" 2>/dev/null || \
        wget -qO "${GLOBAL_SKILLS_DIR}/SKILL.md" "${RAW_BASE_URL}/SKILL.md" 2>/dev/null || \
        echo "⚠️ Warning: Could not fetch SKILL.md from ${RAW_BASE_URL}" >&2
    fi

    echo "📚 Downloading reference documentation from GitHub (${REF})..."
    for doc in antigravity_lifecycle.md how_it_works.md omp_comparison.md; do
        if command -v curl >/dev/null 2>&1; then
            curl --connect-timeout 10 -fsSL "${RAW_BASE_URL}/references/${doc}" -o "${GLOBAL_SKILLS_DIR}/references/${doc}" 2>/dev/null || true
        elif command -v wget >/dev/null 2>&1; then
            wget -qO "${GLOBAL_SKILLS_DIR}/references/${doc}" "${RAW_BASE_URL}/references/${doc}" 2>/dev/null || true
        fi
    done
fi

# 3. Configure Background Hooks in ~/.gemini/config/hooks.json
HOOK_BIN="${INSTALL_DIR}/${BIN_NAME}"
mkdir -p "$(dirname "${HOOKS_CONFIG}")"

echo "⚙️ Configuring background PreInvocation hook (preserving existing user hooks)..."
if [ -f "${HOOKS_CONFIG}" ]; then
    HOOK_BACKUP="${HOOKS_CONFIG}.bak"
    if [ -f "${HOOK_BACKUP}" ]; then
        HOOK_BACKUP="${HOOKS_CONFIG}.bak.$(date +%s)"
    fi
    cp "${HOOKS_CONFIG}" "${HOOK_BACKUP}" 2>/dev/null || true
    EXISTING_CONTENT="$(cat "${HOOKS_CONFIG}")"
    if [ -z "${EXISTING_CONTENT// }" ]; then
        EXISTING_CONTENT="{}"
    fi
else
    EXISTING_CONTENT="{}"
fi

HOOK_MERGED=1

if command -v jq >/dev/null 2>&1; then
    echo "${EXISTING_CONTENT}" | jq --arg bin "${HOOK_BIN} --hook" '
        del(."shake-anchor") |
        .hooks = (.hooks // {}) |
        .hooks.PreInvocation = (
            (((.hooks.PreInvocation // []) | if type == "array" then . else [] end) | map(select((.command != $bin) and (.command | contains("shake-prune") | not)))) +
            [{"command": $bin}]
        ) |
        .hooks.Stop = (
            (((.hooks.Stop // []) | if type == "array" then . else [] end) | map(select((.command != $bin) and (.command | contains("shake-prune") | not)))) +
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
    echo "❌ Error: Neither jq nor python3 was found; cannot merge hooks into ${HOOKS_CONFIG}." >&2
    echo "   Install jq or python3, then re-run ./install.sh (your existing hooks were backed up, nothing was removed)." >&2
    HOOK_MERGED=0
fi

echo ""
echo "🔍 Verifying installation..."
"${INSTALL_DIR}/${BIN_NAME}" --version || { echo "❌ Error: installed binary failed --version check." >&2; exit 1; }
"${INSTALL_DIR}/${BIN_NAME}" doctor --json >/dev/null || { echo "❌ Error: installed binary failed 'doctor --json' check." >&2; exit 1; }

if [ "${HOOK_MERGED}" = "1" ]; then
    echo ""
    echo "🎉 Installation Complete!"
    echo "• Binary installed to: ${INSTALL_DIR}/${BIN_NAME}"
    echo "• /shake skill installed to: ${GLOBAL_SKILLS_DIR}"
    echo "• Proactive auto-compaction hook configured in: ${HOOKS_CONFIG}"
    echo ""
    echo "👉 Type /shake in any Antigravity conversation to compact context!"
    echo "👉 To uninstall: run ./install.sh --uninstall (or re-run piped with --uninstall)"
else
    echo ""
    echo "⚠️ Binary verified, but hook merge was SKIPPED (see error above)." >&2
    echo "• Binary installed to: ${INSTALL_DIR}/${BIN_NAME}"
    echo "• /shake skill installed to: ${GLOBAL_SKILLS_DIR}"
    exit 1
fi
