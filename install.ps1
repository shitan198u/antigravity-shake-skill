# ==============================================================================
#  Antigravity /shake Skill & Native In-Window Hook Installer (Windows PowerShell)
# ==============================================================================
param(
    [switch]$Uninstall
)

$ErrorActionPreference = "Stop"

$Repo = "shitan198u/antigravity-shake-skill"
$DefaultTag = "v0.1.10"
$UserHome = [System.Environment]::GetFolderPath([System.Environment+SpecialFolder]::UserProfile)
$GlobalSkillsDir = Join-Path $UserHome ".gemini\config\skills\shake"
$FullShakeSkillsDir = Join-Path $UserHome ".gemini\config\skills\full-shake"
$GlobalBinDir = Join-Path $UserHome ".gemini\bin"
$HooksConfig = Join-Path $UserHome ".gemini\config\hooks.json"
$TargetExe = Join-Path $GlobalBinDir "shake-prune.exe"

# ==============================================================================
# UNINSTALL MODE
# ==============================================================================
if ($Uninstall) {
    Write-Host "[*] Uninstalling Antigravity /shake and /full-shake..." -ForegroundColor Cyan

    if (Test-Path $TargetExe) {
        Remove-Item -Force $TargetExe
        Write-Host "  [OK] Removed $TargetExe"
    }
    if (Test-Path $GlobalSkillsDir) {
        Remove-Item -Recurse -Force $GlobalSkillsDir
        Write-Host "  [OK] Removed $GlobalSkillsDir"
    }
    if (Test-Path $FullShakeSkillsDir) {
        Remove-Item -Recurse -Force $FullShakeSkillsDir
        Write-Host "  [OK] Removed $FullShakeSkillsDir"
    }

    if (Test-Path $HooksConfig) {
        try {
            $HooksObj = Get-Content $HooksConfig -Raw | ConvertFrom-Json
            if ($HooksObj.hooks -and $HooksObj.hooks.PreInvocation) {
                $Filtered = @()
                foreach ($h in $HooksObj.hooks.PreInvocation) {
                    if ($h.command -notmatch "shake-prune") {
                        $Filtered += $h
                    }
                }
                $HooksObj.hooks.PreInvocation = $Filtered
                $HooksObj | ConvertTo-Json -Depth 5 | Set-Content $HooksConfig -Encoding UTF8
                Write-Host "  [OK] Cleaned PreInvocation hook from $HooksConfig"
            }
        } catch {
            Write-Warning "Could not update hooks.json: $_"
        }
    }

    Write-Host "[DONE] Antigravity /shake has been completely uninstalled." -ForegroundColor Green
    exit 0
}

# ==============================================================================
# INSTALL MODE
# ==============================================================================
Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host "          [*] Antigravity /shake Skill & Native Hook Installation [*]" -ForegroundColor Cyan
Write-Host "================================================================================" -ForegroundColor Cyan

New-Item -ItemType Directory -Force -Path (Join-Path $GlobalSkillsDir "bin") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $GlobalSkillsDir "references") | Out-Null
New-Item -ItemType Directory -Force -Path $FullShakeSkillsDir | Out-Null
New-Item -ItemType Directory -Force -Path $GlobalBinDir | Out-Null

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

# 1. Install SKILL.md and documentation
Write-Host "- Installing skill definitions to: $GlobalSkillsDir"
if (Test-Path (Join-Path $ScriptDir "skills\shake\SKILL.md")) {
    Copy-Item (Join-Path $ScriptDir "skills\shake\SKILL.md") (Join-Path $GlobalSkillsDir "SKILL.md") -Force
}
if (Test-Path (Join-Path $ScriptDir "skills\full-shake\SKILL.md")) {
    Copy-Item (Join-Path $ScriptDir "skills\full-shake\SKILL.md") (Join-Path $FullShakeSkillsDir "SKILL.md") -Force
}
if (Test-Path (Join-Path $ScriptDir "references")) {
    Copy-Item (Join-Path $ScriptDir "references\*") (Join-Path $GlobalSkillsDir "references") -Recurse -Force
}

# 2. Install Native Precompiled Binary
$InstalledBinary = $false

if (Test-Path (Join-Path $ScriptDir "bin\shake-prune.exe")) {
    Write-Host "- Installing local compiled native binary from bin/..."
    Copy-Item (Join-Path $ScriptDir "bin\shake-prune.exe") $TargetExe -Force
    Copy-Item (Join-Path $ScriptDir "bin\shake-prune.exe") (Join-Path $GlobalSkillsDir "bin\shake-prune.exe") -Force
    $InstalledBinary = $true
}

if (-not $InstalledBinary) {
    $DownloadFile = "shake-prune-windows-x86_64.exe"
    $Version = if ($env:SHAKE_VERSION) { $env:SHAKE_VERSION } else { "latest" }
    $BaseReleaseUrl = if ($Version -eq "latest") {
        "https://github.com/$Repo/releases/latest/download"
    } else {
        "https://github.com/$Repo/releases/download/$Version"
    }

    $TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Force -Path $TempDir | Out-Null

    $TempExe = Join-Path $TempDir "shake-prune.exe"
    $TempSums = Join-Path $TempDir "SHA256SUMS.txt"

    try {
        Write-Host "- Downloading precompiled release binary ($DownloadFile) from $BaseReleaseUrl..."
        Invoke-WebRequest -Uri "$BaseReleaseUrl/$DownloadFile" -OutFile $TempExe -UseBasicParsing
        Invoke-WebRequest -Uri "$BaseReleaseUrl/SHA256SUMS.txt" -OutFile $TempSums -UseBasicParsing

        Write-Host "- Verifying SHA256 integrity checksum..."
        if (-not (Test-Path $TempSums)) {
            throw "SHA256SUMS.txt could not be retrieved from $BaseReleaseUrl"
        }

        $SumsLines = Get-Content $TempSums
        $ExpectedHash = ""
        foreach ($line in $SumsLines) {
            if ($line -match "^([a-fA-F0-9]{64})\s+(\*)?$DownloadFile$") {
                $ExpectedHash = $Matches[1].ToLower()
                break
            }
        }

        if (-not $ExpectedHash) {
            throw "Asset $DownloadFile was not found in SHA256SUMS.txt"
        }

        $ActualHash = (Get-FileHash $TempExe -Algorithm SHA256).Hash.ToLower()

        if ($ExpectedHash -ne $ActualHash) {
            throw "SHA256 checksum mismatch! Expected $ExpectedHash, got $ActualHash. Possible download corruption."
        }

        Write-Host "  [OK] SHA256 checksum verified: $ActualHash" -ForegroundColor Green
        Copy-Item $TempExe $TargetExe -Force
        Copy-Item $TempExe (Join-Path $GlobalSkillsDir "bin\shake-prune.exe") -Force
        $InstalledBinary = $true
    } catch {
        Write-Warning "Precompiled binary verification failed: $_"
    } finally {
        Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue
    }

    if (-not $InstalledBinary -and (Get-Command "cargo" -ErrorAction SilentlyContinue)) {
        Write-Host "- Building native Rust binary from source via Cargo..."
        cargo build --release --manifest-path (Join-Path $ScriptDir "shake-prune-rs\Cargo.toml")
        Copy-Item (Join-Path $ScriptDir "shake-prune-rs\target\release\shake-prune.exe") $TargetExe -Force
        Copy-Item (Join-Path $ScriptDir "shake-prune-rs\target\release\shake-prune.exe") (Join-Path $GlobalSkillsDir "bin\shake-prune.exe") -Force
        $InstalledBinary = $true
    }
}

if (-not $InstalledBinary) {
    Write-Error "Installation Failed: shake-prune binary could not be installed. Release download failed and local Cargo is not available. Please install Rust or check internet connectivity."
    exit 1
}

# 3. Safely merge PreInvocation hook into hooks.json
Write-Host "- Merging PreInvocation hook into ~/.gemini/config/hooks.json (preserving existing hooks)..."
$EscapedHookExe = $TargetExe.Replace("\", "\\")
$HookCommand = "$EscapedHookExe --hook"

if (Test-Path $HooksConfig) {
    Copy-Item $HooksConfig "$HooksConfig.bak" -Force -ErrorAction SilentlyContinue
}

$HooksObj = @{ "hooks" = @{ "PreInvocation" = @() } }
if (Test-Path $HooksConfig) {
    try {
        $HooksObj = Get-Content $HooksConfig -Raw | ConvertFrom-Json
        if ($HooksObj.PSObject.Properties["shake-anchor"]) {
            $HooksObj.PSObject.Properties.Remove("shake-anchor")
        }
        if (-not $HooksObj.hooks) { $HooksObj | Add-Member -MemberType NoteProperty -Name "hooks" -Value @{} }
        if (-not $HooksObj.hooks.PreInvocation) { $HooksObj.hooks | Add-Member -MemberType NoteProperty -Name "PreInvocation" -Value @() }
    } catch {
        Write-Warning "Could not parse existing hooks.json, creating a new one."
    }
}

$NewPreInvocation = @()
foreach ($h in $HooksObj.hooks.PreInvocation) {
    if ($h.command -notmatch "shake-prune") {
        $NewPreInvocation += $h
    }
}
$NewPreInvocation += @{ "command" = $HookCommand }
$HooksObj.hooks.PreInvocation = $NewPreInvocation

$HooksObj | ConvertTo-Json -Depth 5 | Set-Content $HooksConfig -Encoding UTF8

Write-Host "--------------------------------------------------------------------------------" -ForegroundColor Green
Write-Host "[OK] Installation complete!" -ForegroundColor Green
Write-Host "- Pure native Rust binary installed at: $TargetExe"
Write-Host "- Skill and In-Window Anchor are globally active."
Write-Host "- To use it: Type '/shake' or '/full-shake' in any Antigravity conversation and keep chatting in the same tab!"
Write-Host "================================================================================" -ForegroundColor Cyan
