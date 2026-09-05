# ==============================================================================
#  Antigravity /shake Skill & Native In-Window Hook Installer (Windows PowerShell)
# ==============================================================================
param(
    [switch]$Uninstall
)

$ErrorActionPreference = "Stop"

$Repo = "shitan198u/antigravity-shake-skill"
$DefaultTag = "v0.2.0"
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
    Write-Host "[*] Uninstalling Antigravity /shake..." -ForegroundColor Cyan

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
            $Modified = $false
            if ($HooksObj.hooks -and $HooksObj.hooks.PreInvocation) {
                $Filtered = @()
                foreach ($h in $HooksObj.hooks.PreInvocation) {
                    if ($h.command -notmatch "shake-prune") {
                        $Filtered += $h
                    }
                }
                $HooksObj.hooks.PreInvocation = $Filtered
                $Modified = $true
                Write-Host "  [OK] Cleaned PreInvocation hook from $HooksConfig"
            }
            if ($HooksObj.hooks -and $HooksObj.hooks.Stop) {
                $FilteredStop = @()
                foreach ($h in $HooksObj.hooks.Stop) {
                    if ($h.command -notmatch "shake-prune") {
                        $FilteredStop += $h
                    }
                }
                $HooksObj.hooks.Stop = $FilteredStop
                $Modified = $true
                Write-Host "  [OK] Cleaned Stop hook from $HooksConfig"
            }
            if ($Modified) {
                $HooksObj | ConvertTo-Json -Depth 5 | Set-Content $HooksConfig -Encoding UTF8
            }
        } catch {
            Write-Warning "Could not update hooks.json: $_"
        }
    }

    Write-Host "[DONE] Antigravity /shake binaries, skills, and hooks removed." -ForegroundColor Green
    Write-Host "Retained (delete manually if desired): shake.toml, logs, transcript_full.jsonl archives, and .bak files."
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
New-Item -ItemType Directory -Force -Path $GlobalBinDir | Out-Null

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

# 1. Install SKILL.md and documentation
Write-Host "- Installing skill definitions to: $GlobalSkillsDir"
if (Test-Path (Join-Path $ScriptDir "skills\shake\SKILL.md")) {
    Copy-Item (Join-Path $ScriptDir "skills\shake\SKILL.md") (Join-Path $GlobalSkillsDir "SKILL.md") -Force
} else {
    Write-Warning "No SKILL.md found in $ScriptDir\skills\shake; skill text skipped."
}
if (Test-Path (Join-Path $ScriptDir "references")) {
    Copy-Item (Join-Path $ScriptDir "references\*") (Join-Path $GlobalSkillsDir "references") -Recurse -Force
} else {
    Write-Warning "No references directory in $ScriptDir; references skipped."
}

# 2. Install Native Precompiled Binary
$InstalledBinary = $false

if (Test-Path (Join-Path $ScriptDir "bin\shake-prune.exe")) {
    Write-Host "- Installing local compiled native binary from bin/..."
    Copy-Item (Join-Path $ScriptDir "bin\shake-prune.exe") $TargetExe -Force
    Copy-Item (Join-Path $ScriptDir "bin\shake-prune.exe") (Join-Path $GlobalSkillsDir "bin\shake-prune.exe") -Force
    $InstalledBinary = $true
}

if ((-not $InstalledBinary) -and (Test-Path (Join-Path $ScriptDir "shake-prune-rs\target\release\shake-prune.exe"))) {
    Write-Host "- Installing local cargo release binary from shake-prune-rs\target\release\..."
    Copy-Item (Join-Path $ScriptDir "shake-prune-rs\target\release\shake-prune.exe") $TargetExe -Force
    Copy-Item (Join-Path $ScriptDir "shake-prune-rs\target\release\shake-prune.exe") (Join-Path $GlobalSkillsDir "bin\shake-prune.exe") -Force
    $InstalledBinary = $true
}

if (-not $InstalledBinary) {
    $Arch = if ($env:PROCESSOR_ARCHITECTURE) { $env:PROCESSOR_ARCHITECTURE.ToLower() } else { "amd64" }
    $DownloadFile = if ($Arch -eq "arm64") {
        Write-Host "- Detected Windows ARM64 architecture (using Windows x64 binary via emulation)..."
        "shake-prune-windows-x86_64.exe"
    } else {
        "shake-prune-windows-x86_64.exe"
    }
    $Version = if ($env:SHAKE_VERSION) { $env:SHAKE_VERSION } else { $DefaultTag }
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
        $EscapedFile = [regex]::Escape($DownloadFile)
        $Pattern = "^([a-fA-F0-9]{64})\s+(\*)?" + $EscapedFile + "\s*$"
        foreach ($line in $SumsLines) {
            if ($line -match $Pattern) {
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
        Write-Warning "Precompiled binary download/verification failed: $_"
        Write-Host "   If you have Rust installed, you can build from source: cargo build --release --manifest-path shake-prune-rs\Cargo.toml" -ForegroundColor Yellow
    } finally {
        Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue
    }
}

if (-not $InstalledBinary) {
    Write-Error "Installation Failed: shake-prune binary could not be installed. Release download failed. Please check internet connectivity or build from source with: cargo build --release --manifest-path shake-prune-rs\Cargo.toml"
    exit 1
}

# 3. Safely merge PreInvocation and Stop hooks into hooks.json
Write-Host "- Merging PreInvocation and Stop hooks into ~/.gemini/config/hooks.json (preserving existing hooks)..."
# NOTE: Do not pre-escape backslashes; ConvertTo-Json handles JSON escaping.
$HookCommand = "$TargetExe --hook"

if (Test-Path $HooksConfig) {
    $HookBackup = "$HooksConfig.bak"
    if (Test-Path $HookBackup) { $HookBackup = "$HooksConfig.bak.$([int][double]::Parse((Get-Date -UFormat %s)))" }
    Copy-Item $HooksConfig $HookBackup -Force -ErrorAction SilentlyContinue
}

$HooksObj = @{ "hooks" = @{ "PreInvocation" = @(); "Stop" = @() } }
if (Test-Path $HooksConfig) {
    try {
        $HooksObj = Get-Content $HooksConfig -Raw | ConvertFrom-Json
        if ($HooksObj.PSObject.Properties["shake-anchor"]) {
            $HooksObj.PSObject.Properties.Remove("shake-anchor")
        }
        if (-not $HooksObj.hooks) { $HooksObj | Add-Member -MemberType NoteProperty -Name "hooks" -Value @{} }
        if (-not $HooksObj.hooks.PreInvocation) { $HooksObj.hooks | Add-Member -MemberType NoteProperty -Name "PreInvocation" -Value @() }
        if (-not $HooksObj.hooks.Stop) { $HooksObj.hooks | Add-Member -MemberType NoteProperty -Name "Stop" -Value @() }
        # Coerce non-array values (single object, string) to arrays before filtering.
        if ($HooksObj.hooks.PreInvocation -isnot [array]) { $HooksObj.hooks.PreInvocation = @($HooksObj.hooks.PreInvocation) }
        if ($HooksObj.hooks.Stop -isnot [array]) { $HooksObj.hooks.Stop = @($HooksObj.hooks.Stop) }
    } catch {
        Write-Warning "Could not parse existing hooks.json, creating a new one."
    }
}

$NewPreInvocation = @()
foreach ($h in @($HooksObj.hooks.PreInvocation)) {
    if ($h -is [string]) { continue }
    if ($h.command -notmatch "shake-prune") {
        $NewPreInvocation += $h
    }
}
$NewPreInvocation += @{ "command" = $HookCommand }
$HooksObj.hooks.PreInvocation = $NewPreInvocation

$NewStop = @()
foreach ($h in @($HooksObj.hooks.Stop)) {
    if ($h -is [string]) { continue }
    if ($h.command -notmatch "shake-prune") {
        $NewStop += $h
    }
}
$NewStop += @{ "command" = $HookCommand }
$HooksObj.hooks.Stop = $NewStop

$HooksObj | ConvertTo-Json -Depth 5 | Set-Content $HooksConfig -Encoding UTF8

Write-Host "- Verifying installation..."
& $TargetExe --version
if ($LASTEXITCODE -ne 0) { Write-Error "Installed binary failed --version check."; exit 1 }
& $TargetExe doctor --json | Out-Null
if ($LASTEXITCODE -ne 0) { Write-Error "Installed binary failed 'doctor --json' check."; exit 1 }
