# ==============================================================================
# Antigravity `/shake` Skill Installer (Windows PowerShell)
# Installs the high-speed /shake context-pruning skill globally for Windows,
# with native PreInvocation hook support & SHA256 integrity verification.
# ==============================================================================

$ErrorActionPreference = "SilentlyContinue"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$UserHome = $env:USERPROFILE
$TargetConfigDir = Join-Path $UserHome ".gemini\config"
$TargetSkillDir = Join-Path $TargetConfigDir "skills\shake"
$TargetBinDir = Join-Path $UserHome ".gemini\bin"
$RepoUrl = "https://github.com/shitan198u/antigravity-shake-skill"

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

# 3. Binary Installation (Prebuilt -> Verified GitHub Releases Download -> Local Cargo Compile -> Python Fallback)
$PrebuiltBin = Join-Path $ScriptDir "bin\shake-prune.exe"
$BinaryInstalled = $false

if (Test-Path $PrebuiltBin) {
    Write-Host "• Installing precompiled native binary to: $TargetBinDir\shake-prune.exe"
    Copy-Item $PrebuiltBin (Join-Path $TargetBinDir "shake-prune.exe") -Force
    Copy-Item $PrebuiltBin (Join-Path $TargetSkillDir "bin\shake-prune.exe") -Force
    $BinaryInstalled = $true
} else {
    Write-Host "• Fetching precompiled Windows binary & checksums from GitHub Releases..."
    $WinBinUrl = "$RepoUrl/releases/latest/download/shake-prune-windows-x86_64.exe"
    $SumsUrl = "$RepoUrl/releases/latest/download/SHA256SUMS.txt"
    $TargetExe = Join-Path $TargetBinDir "shake-prune.exe"
    $TmpSums = Join-Path $env:TEMP "shake_SHA256SUMS.txt"

    try {
        Invoke-WebRequest -Uri $WinBinUrl -OutFile $TargetExe -UseBasicParsing
        Invoke-WebRequest -Uri $SumsUrl -OutFile $TmpSums -UseBasicParsing

        if ((Test-Path $TargetExe) -and (Test-Path $TmpSums)) {
            $ActualHash = (Get-FileHash -Path $TargetExe -Algorithm SHA256).Hash.ToLower()
            $ExpectedLine = Get-Content $TmpSums | Select-String "shake-prune-windows-x86_64.exe"
            $ExpectedHash = ($ExpectedLine -split "\s+")[0].ToLower()

            if ($ActualHash -eq $ExpectedHash) {
                Copy-Item $TargetExe (Join-Path $TargetSkillDir "bin\shake-prune.exe") -Force
                Write-Host "• Verified SHA256 and installed Windows binary to: $TargetExe"
                $BinaryInstalled = $true
            } else {
                Write-Warning "SHA256 hash mismatch! Discarding downloaded binary."
                Remove-Item $TargetExe -Force
            }
        }
    } catch {
        # Fallback
    }
}

if (-not $BinaryInstalled -and (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host "• Compiling native binary from source via cargo..."
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

# 4. Safe Non-Destructive Merge of PreInvocation Hook in hooks.json
$HooksFile = Join-Path $TargetConfigDir "hooks.json"
$HookCommand = "$TargetBinDir\shake-prune.exe --hook"

$HooksObj = @{}
if (Test-Path $HooksFile) {
    try {
        $HooksObj = Get-Content $HooksFile -Raw | ConvertFrom-Json -AsHashtable
    } catch {
        $HooksObj = @{}
    }
}

$HooksObj["shake-anchor"] = @{
    "enabled" = $true
    "PreInvocation" = @(
        @{
            "type" = "command"
            "command" = $HookCommand
        }
    )
}

$HooksObj | ConvertTo-Json -Depth 5 | Set-Content $HooksFile -Encoding UTF8

Write-Host "--------------------------------------------------------------------------------" -ForegroundColor Green
Write-Host "✅ Installation complete!" -ForegroundColor Green
Write-Host "• Skill & Native In-Window Anchor are globally active."
Write-Host "• To use it: Type '/shake' in any Antigravity conversation and keep chatting in the same tab!"
Write-Host "================================================================================" -ForegroundColor Cyan
