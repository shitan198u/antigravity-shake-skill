# ==============================================================================
#  Antigravity /shake Skill & Native In-Window Hook Installer (Windows PowerShell)
# ==============================================================================
param(
    [switch]$Uninstall,
    [switch]$Local
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

$ScriptDir = if ($MyInvocation.MyCommand.Path) { Split-Path -Parent $MyInvocation.MyCommand.Path } else { $null }
$LocalDev = $Local.IsPresent -or ($env:SHAKE_LOCAL_DEV -eq "1")

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
                $Filtered = @($HooksObj.hooks.PreInvocation | Where-Object { $_.command -notmatch "shake-prune" })
                $HooksObj.hooks.PreInvocation = $Filtered
                $Modified = $true
                Write-Host "  [OK] Cleaned PreInvocation hook from $HooksConfig"
            }
            if ($HooksObj.hooks -and $HooksObj.hooks.Stop) {
                $FilteredStop = @($HooksObj.hooks.Stop | Where-Object { $_.command -notmatch "shake-prune" })
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

$Version = if ($env:SHAKE_VERSION) { $env:SHAKE_VERSION } else { $DefaultTag }
$RefTag = if ($Version -eq "latest") { "main" } else { $Version }
$RawBaseUrl = "https://raw.githubusercontent.com/$Repo/$RefTag"

# 1. Binary acquisition
$InstalledBinary = $false

if ($LocalDev -and $ScriptDir) {
    if (Test-Path (Join-Path $ScriptDir "bin\shake-prune.exe")) {
        Write-Host "- [Local Dev] Installing compiled binary from bin/..."
        Copy-Item (Join-Path $ScriptDir "bin\shake-prune.exe") $TargetExe -Force
        $InstalledBinary = $true
    } elseif (Test-Path (Join-Path $ScriptDir "shake-prune-rs\target\release\shake-prune.exe")) {
        Write-Host "- [Local Dev] Installing cargo release binary from shake-prune-rs\target\release\..."
        Copy-Item (Join-Path $ScriptDir "shake-prune-rs\target\release\shake-prune.exe") $TargetExe -Force
        $InstalledBinary = $true
    }
}

if (-not $InstalledBinary) {
    $DownloadFile = "shake-prune-windows-x86_64.exe"
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
        Invoke-WebRequest -Uri "$BaseReleaseUrl/$DownloadFile" -OutFile $TempExe -UseBasicParsing -TimeoutSec 30
        Invoke-WebRequest -Uri "$BaseReleaseUrl/SHA256SUMS.txt" -OutFile $TempSums -UseBasicParsing -TimeoutSec 15

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
        $InstalledBinary = $true
    } catch {
        Write-Error "Precompiled binary download/verification failed: $_"
        exit 1
    } finally {
        Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue
    }
}

# 2. Skill & Reference deployment (Always fresh overwrite)
Write-Host "- Installing skill definitions to: $GlobalSkillsDir"
Copy-Item $TargetExe (Join-Path $GlobalSkillsDir "bin\shake-prune.exe") -Force

$SkillCopied = $false
if ($LocalDev -and $ScriptDir) {
    if (Test-Path (Join-Path $ScriptDir "skills\shake\SKILL.md")) {
        Write-Host "  -> [Local Dev] Deploying SKILL.md from repository..."
        Copy-Item (Join-Path $ScriptDir "skills\shake\SKILL.md") (Join-Path $GlobalSkillsDir "SKILL.md") -Force
        $SkillCopied = $true
    } elseif (Test-Path (Join-Path $ScriptDir "SKILL.md")) {
        Write-Host "  -> [Local Dev] Deploying SKILL.md from repository..."
        Copy-Item (Join-Path $ScriptDir "SKILL.md") (Join-Path $GlobalSkillsDir "SKILL.md") -Force
        $SkillCopied = $true
    }
    if (Test-Path (Join-Path $ScriptDir "references")) {
        Write-Host "  -> [Local Dev] Deploying references from repository..."
        Copy-Item (Join-Path $ScriptDir "references\*") (Join-Path $GlobalSkillsDir "references") -Recurse -Force
    }
}

if (-not $SkillCopied) {
    Write-Host "  -> Downloading SKILL.md from GitHub ($RefTag)..."
    try {
        Invoke-RestMethod -Uri "$RawBaseUrl/skills/shake/SKILL.md" -OutFile (Join-Path $GlobalSkillsDir "SKILL.md") -TimeoutSec 15
    } catch {
        try {
            Invoke-RestMethod -Uri "$RawBaseUrl/SKILL.md" -OutFile (Join-Path $GlobalSkillsDir "SKILL.md") -TimeoutSec 15
        } catch {
            Write-Warning "Could not fetch SKILL.md from GitHub: $_"
        }
    }

    Write-Host "  -> Downloading references from GitHub ($RefTag)..."
    $RefDocs = @("antigravity_lifecycle.md", "how_it_works.md", "omp_comparison.md")
    foreach ($doc in $RefDocs) {
        try {
            Invoke-RestMethod -Uri "$RawBaseUrl/references/$doc" -OutFile (Join-Path $GlobalSkillsDir "references\$doc") -TimeoutSec 15
        } catch {
            # Non-fatal
        }
    }
}

# 3. Configure Background PreInvocation + Stop hooks in hooks.json
Write-Host "- Merging PreInvocation and Stop hooks into ~/.gemini/config/hooks.json..."
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

# 5. Verification
Write-Host "- Verifying installation..."
& $TargetExe --version
& $TargetExe doctor --json | Out-Null

Write-Host ""
Write-Host "[DONE] Installation Complete!" -ForegroundColor Green
Write-Host "- Binary installed to: $TargetExe"
Write-Host "- Skill installed to: $GlobalSkillsDir"
Write-Host "- Native hooks configured in: $HooksConfig"
Write-Host "Type /shake in any conversation to compact context!" -ForegroundColor Cyan
Write-Host "To uninstall: powershell -File .\install.ps1 -Uninstall" -ForegroundColor Cyan
