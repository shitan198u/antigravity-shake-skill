# ==============================================================================
#  Antigravity /shake Skill & Native In-Window Hook Installer (Windows PowerShell)
# ==============================================================================

$ErrorActionPreference = "Stop"

$RepoUrl = "https://github.com/shitan198u/antigravity-shake-skill"
$ReleaseTag = "v0.1.4"
$UserHome = [System.Environment]::GetFolderPath([System.Environment+SpecialFolder]::UserProfile)
$GlobalSkillsDir = Join-Path $UserHome ".gemini\config\skills\shake"
$FullShakeSkillsDir = Join-Path $UserHome ".gemini\config\skills\full-shake"
$GlobalBinDir = Join-Path $UserHome ".gemini\bin"
$HooksConfig = Join-Path $UserHome ".gemini\config\hooks.json"

Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host "          ⚡ Antigravity /shake Skill & Native Hook Installation ⚡" -ForegroundColor Cyan
Write-Host "================================================================================" -ForegroundColor Cyan

New-Item -ItemType Directory -Force -Path (Join-Path $GlobalSkillsDir "bin") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $GlobalSkillsDir "references") | Out-Null
New-Item -ItemType Directory -Force -Path $FullShakeSkillsDir | Out-Null
New-Item -ItemType Directory -Force -Path $GlobalBinDir | Out-Null

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

# 1. Install SKILL.md and documentation
Write-Host "• Installing skill definition to: $GlobalSkillsDir"
Copy-Item (Join-Path $ScriptDir "SKILL.md") (Join-Path $GlobalSkillsDir "SKILL.md") -Force
Copy-Item (Join-Path $ScriptDir "skills\full-shake\SKILL.md") (Join-Path $FullShakeSkillsDir "SKILL.md") -Force
Copy-Item (Join-Path $ScriptDir "references\*") (Join-Path $GlobalSkillsDir "references") -Recurse -Force

# 2. Install Native Precompiled Binary
$TargetExe = Join-Path $GlobalBinDir "shake-prune.exe"
$InstalledBinary = $false

if (Test-Path (Join-Path $ScriptDir "bin\shake-prune.exe")) {
    Write-Host "• Installing local compiled native binary..."
    Copy-Item (Join-Path $ScriptDir "bin\shake-prune.exe") $TargetExe -Force
    Copy-Item (Join-Path $ScriptDir "bin\shake-prune.exe") (Join-Path $GlobalSkillsDir "bin\shake-prune.exe") -Force
    $InstalledBinary = $true
}

if (-not $InstalledBinary) {
    $DownloadFile = "shake-prune-windows-x86_64.exe"
    $BaseReleaseUrl = "$RepoUrl/releases/download/$ReleaseTag"
    $TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Force -Path $TempDir | Out-Null

    $TempExe = Join-Path $TempDir "shake-prune.exe"
    $TempSums = Join-Path $TempDir "SHA256SUMS.txt"

    try {
        Write-Host "• Downloading precompiled release binary ($DownloadFile) from GitHub..."
        Invoke-WebRequest -Uri "$BaseReleaseUrl/$DownloadFile" -OutFile $TempExe -UseBasicParsing
        Invoke-WebRequest -Uri "$BaseReleaseUrl/SHA256SUMS.txt" -OutFile $TempSums -UseBasicParsing

        Write-Host "• Verifying SHA256 integrity checksum..."
        $SumsLines = Get-Content $TempSums
        $ExpectedHash = ""
        foreach ($line in $SumsLines) {
            if ($line -match "^([a-fA-F0-9]{64})\s+(\*)?shake-prune-windows-x86_64\.exe$") {
                $ExpectedHash = $Matches[1].ToLower()
                break
            }
        }

        $ActualHash = (Get-FileHash $TempExe -Algorithm SHA256).Hash.ToLower()

        if ($ExpectedHash -and ($ExpectedHash -eq $ActualHash)) {
            Write-Host "  ✓ SHA256 checksum verified: $ActualHash" -ForegroundColor Green
            Copy-Item $TempExe $TargetExe -Force
            Copy-Item $TempExe (Join-Path $GlobalSkillsDir "bin\shake-prune.exe") -Force
            $InstalledBinary = $true
        } else {
            Write-Warning "SHA256 checksum mismatch! Building from local source..."
        }
    } catch {
        Write-Warning "Could not download precompiled binary: $_"
    } finally {
        Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue
    }

    if (-not $InstalledBinary -and (Get-Command "cargo" -ErrorAction SilentlyContinue)) {
        Write-Host "• Building native Rust binary from source..."
        cargo build --release --manifest-path (Join-Path $ScriptDir "shake-prune-rs\Cargo.toml")
        Copy-Item (Join-Path $ScriptDir "shake-prune-rs\target\release\shake-prune.exe") $TargetExe -Force
        Copy-Item (Join-Path $ScriptDir "shake-prune-rs\target\release\shake-prune.exe") (Join-Path $GlobalSkillsDir "bin\shake-prune.exe") -Force
        $InstalledBinary = $true
    }
}

# 3. Safely merge PreInvocation hook into hooks.json
Write-Host "• Merging PreInvocation hook into ~/.gemini/config/hooks.json (preserving existing hooks)..."
$EscapedHookExe = $TargetExe.Replace("\", "\\")
$HookCommand = "$EscapedHookExe --hook"

$HooksObj = @{ "hooks" = @{ "PreInvocation" = @() } }
if (Test-Path $HooksConfig) {
    try {
        $HooksObj = Get-Content $HooksConfig -Raw | ConvertFrom-Json
        if (-not $HooksObj.hooks) { $HooksObj | Add-Member -MemberType NoteProperty -Name "hooks" -Value @{} }
        if (-not $HooksObj.hooks.PreInvocation) { $HooksObj.hooks | Add-Member -MemberType NoteProperty -Name "PreInvocation" -Value @() }
    } catch {
        Write-Warning "Could not parse existing hooks.json, creating a new one."
    }
}

$NewPreInvocation = @()
foreach ($h in $HooksObj.hooks.PreInvocation) {
    if ($h.name -ne "shake-anchor") {
        $NewPreInvocation += $h
    }
}
$NewPreInvocation += @{ "name" = "shake-anchor"; "command" = $HookCommand }
$HooksObj.hooks.PreInvocation = $NewPreInvocation

$HooksObj | ConvertTo-Json -Depth 5 | Set-Content $HooksConfig -Encoding UTF8

Write-Host "--------------------------------------------------------------------------------" -ForegroundColor Green
Write-Host "✅ Installation complete!" -ForegroundColor Green
Write-Host "• Pure native Rust binary installed at: $TargetExe"
Write-Host "• Skill & In-Window Anchor are globally active."
Write-Host "• To use it: Type '/shake' in any Antigravity conversation and keep chatting in the same tab!"
Write-Host "================================================================================" -ForegroundColor Cyan
