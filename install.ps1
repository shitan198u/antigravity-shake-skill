# ==============================================================================
# Antigravity `/shake` Skill Installer (Windows PowerShell)
# Installs the high-speed /shake context-pruning skill globally for Windows.
# ==============================================================================

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$UserHome = $env:USERPROFILE
$TargetConfigDir = Join-Path $UserHome ".gemini\config"
$TargetSkillDir = Join-Path $TargetConfigDir "skills\shake"
$TargetBinDir = Join-Path $UserHome ".gemini\bin"

Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host "          ⚡ Antigravity /shake Skill & Native Hook Installation (Windows) ⚡" -ForegroundColor Cyan
Write-Host "================================================================================" -ForegroundColor Cyan

# 1. Create Directories
New-Item -ItemType Directory -Force -Path (Join-Path $TargetSkillDir "scripts") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $TargetSkillDir "references") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $TargetSkillDir "assets") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $TargetSkillDir "bin") | Out-Null
New-Item -ItemType Directory -Force -Path $TargetBinDir | Out-Null

# 2. Copy Skill definitions & scripts
Write-Host "• Installing skill definition to: $TargetSkillDir"
Copy-Item (Join-Path $ScriptDir "SKILL.md") (Join-Path $TargetSkillDir "SKILL.md") -Force
Copy-Item (Join-Path $ScriptDir "scripts\shake_prune.py") (Join-Path $TargetSkillDir "scripts\shake_prune.py") -Force
Copy-Item (Join-Path $ScriptDir "references\omp_comparison.md") (Join-Path $TargetSkillDir "references\omp_comparison.md") -Force

if (Test-Path (Join-Path $ScriptDir "assets\artifact_preview.png")) {
    Copy-Item (Join-Path $ScriptDir "assets\artifact_preview.png") (Join-Path $TargetSkillDir "assets\artifact_preview.png") -Force
}

# 3. Binary Installation (Prebuilt -> Local Compile -> Python Fallback)
$PrebuiltBin = Join-Path $ScriptDir "bin\shake-prune.exe"
$BinaryInstalled = $false

if (Test-Path $PrebuiltBin) {
    Write-Host "• Installing precompiled native binary to: $TargetBinDir\shake-prune.exe"
    Copy-Item $PrebuiltBin (Join-Path $TargetBinDir "shake-prune.exe") -Force
    Copy-Item $PrebuiltBin (Join-Path $TargetSkillDir "bin\shake-prune.exe") -Force
    $BinaryInstalled = $true
} elseif (Get-Command cargo -ErrorAction SilentlyContinue) {
    Write-Host "• Compiling native binary via cargo..."
    cargo build --release --manifest-path (Join-Path $ScriptDir "shake-prune-rs\Cargo.toml")
    $CompiledBin = Join-Path $ScriptDir "shake-prune-rs\target\release\shake-prune.exe"
    if (Test-Path $CompiledBin) {
        Copy-Item $CompiledBin (Join-Path $TargetBinDir "shake-prune.exe") -Force
        Copy-Item $CompiledBin (Join-Path $TargetSkillDir "bin\shake-prune.exe") -Force
        $BinaryInstalled = $true
    }
}

if (-not $BinaryInstalled) {
    Write-Host "• Note: Using universal Python fallback engine (scripts/shake_prune.py)."
}

# 4. Register PreInvocation Hook in hooks.json
$HooksFile = Join-Path $TargetConfigDir "hooks.json"
$HookCommand = "$TargetBinDir\shake-prune.exe --hook"

$HooksObj = @{
    "shake-anchor" = @{
        "enabled" = $true
        "PreInvocation" = @(
            @{
                "type" = "command"
                "command" = $HookCommand
            }
        )
    }
}

$HooksObj | ConvertTo-Json -Depth 5 | Set-Content $HooksFile -Encoding UTF8

Write-Host "--------------------------------------------------------------------------------" -ForegroundColor Green
Write-Host "✅ Installation complete!" -ForegroundColor Green
Write-Host "• Skill & Native In-Window Anchor are globally active."
Write-Host "• To use it: Type '/shake' in any Antigravity conversation and keep chatting in the same tab!"
Write-Host "================================================================================" -ForegroundColor Cyan
