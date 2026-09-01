#!/usr/bin/env bash
# ==============================================================================
# Antigravity `/shake` Skill Installer
# Installs the high-speed /shake context-pruning skill globally for Antigravity,
# including native PreInvocation hook support for seamless in-window continuation.
# ==============================================================================

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TARGET_CONFIG_DIR="${HOME}/.gemini/config"
TARGET_SKILL_DIR="${TARGET_CONFIG_DIR}/skills/shake"
TARGET_BIN_DIR="${HOME}/.gemini/bin"

echo "================================================================================"
echo "          ⚡ Antigravity /shake Skill & Hook Installation ⚡"
echo "================================================================================"

# 1. Ensure target directories exist
mkdir -p "${TARGET_SKILL_DIR}/scripts"
mkdir -p "${TARGET_SKILL_DIR}/references"
mkdir -p "${TARGET_SKILL_DIR}/assets"
mkdir -p "${TARGET_BIN_DIR}"

# 2. Copy Skill definition, fallback scripts, assets, and reference documentation
echo "• Installing skill definition to: ${TARGET_SKILL_DIR}"
cp "${SCRIPT_DIR}/SKILL.md" "${TARGET_SKILL_DIR}/SKILL.md"
cp "${SCRIPT_DIR}/scripts/shake_prune.py" "${TARGET_SKILL_DIR}/scripts/shake_prune.py"
chmod +x "${TARGET_SKILL_DIR}/scripts/shake_prune.py"
cp "${SCRIPT_DIR}/scripts/pre_invocation_hook.py" "${TARGET_SKILL_DIR}/scripts/pre_invocation_hook.py"
chmod +x "${TARGET_SKILL_DIR}/scripts/pre_invocation_hook.py"
cp "${SCRIPT_DIR}/references/omp_comparison.md" "${TARGET_SKILL_DIR}/references/omp_comparison.md"

if [ -f "${SCRIPT_DIR}/assets/artifact_preview.png" ]; then
    cp "${SCRIPT_DIR}/assets/artifact_preview.png" "${TARGET_SKILL_DIR}/assets/artifact_preview.png"
fi

# 3. Install Binary (Prebuilt -> Local Compile -> Python Fallback)
PREBUILT_BIN="${SCRIPT_DIR}/bin/shake-prune"
BINARY_INSTALLED=false

if [ -f "${PREBUILT_BIN}" ] && "${PREBUILT_BIN}" --help >/dev/null 2>&1; then
    echo "• Installing precompiled native binary to: ${TARGET_BIN_DIR}/shake-prune"
    cp "${PREBUILT_BIN}" "${TARGET_BIN_DIR}/shake-prune"
    chmod +x "${TARGET_BIN_DIR}/shake-prune"
    BINARY_INSTALLED=true
elif command -v cargo >/dev/null 2>&1 && [ -f "${SCRIPT_DIR}/shake-prune-rs/Cargo.toml" ]; then
    echo "• Prebuilt binary incompatible with this architecture/glibc. Compiling via cargo..."
    cargo build --release --manifest-path "${SCRIPT_DIR}/shake-prune-rs/Cargo.toml"
    RUST_BIN="${SCRIPT_DIR}/shake-prune-rs/target/release/shake-prune"
    if [ -f "${RUST_BIN}" ]; then
        cp "${RUST_BIN}" "${TARGET_BIN_DIR}/shake-prune"
        chmod +x "${TARGET_BIN_DIR}/shake-prune"
        echo "• Installed compiled native binary to: ${TARGET_BIN_DIR}/shake-prune"
        BINARY_INSTALLED=true
    fi
fi

if [ "${BINARY_INSTALLED}" = false ]; then
    echo "• Note: Using universal Python fallback engine (scripts/shake_prune.py)."
fi

# 4. Install / Merge PreInvocation Hook in ~/.gemini/config/hooks.json
echo "• Registering PreInvocation lifecycle hook for seamless in-window continuation..."
python3 -c "
import json, os

hooks_file = os.path.expanduser('~/.gemini/config/hooks.json')
hook_script = os.path.expanduser('~/.gemini/config/skills/shake/scripts/pre_invocation_hook.py')

existing = {}
if os.path.exists(hooks_file):
    try:
        with open(hooks_file, 'r') as f:
            existing = json.load(f)
    except Exception:
        existing = {}

existing['shake-anchor'] = {
    'enabled': True,
    'PreInvocation': [
        {
            'type': 'command',
            'command': f'python3 {hook_script}'
        }
    ]
}

with open(hooks_file, 'w') as f:
    json.dump(existing, f, indent=2)
"

echo "--------------------------------------------------------------------------------"
echo "✅ Installation complete!"
echo "• Skill & In-Window Anchor Hook are now globally available across all chats."
echo "• To use it: Type '/shake' in any Antigravity conversation and keep chatting in the same tab!"
echo "================================================================================"
