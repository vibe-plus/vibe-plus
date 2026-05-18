param(
  [string]$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path,
  [string]$ExePath = (Join-Path $RepoRoot "target/release/vibe.exe")
)

$ErrorActionPreference = "Stop"

function Invoke-Vibe {
  param(
    [Parameter(Mandatory = $true)][string]$FilePath,
    [string[]]$Arguments = @(),
    [string]$Stdin = $null,
    [string]$Expected = $null
  )

  $psi = [System.Diagnostics.ProcessStartInfo]::new()
  $psi.FileName = $FilePath
  foreach ($arg in $Arguments) {
    [void]$psi.ArgumentList.Add($arg)
  }
  $psi.WorkingDirectory = $RepoRoot
  $psi.RedirectStandardOutput = $true
  $psi.RedirectStandardError = $true
  $psi.RedirectStandardInput = $null -ne $Stdin
  $psi.UseShellExecute = $false

  $process = [System.Diagnostics.Process]::Start($psi)
  if ($null -ne $Stdin) {
    $process.StandardInput.Write($Stdin)
    $process.StandardInput.Close()
  }

  $stdout = $process.StandardOutput.ReadToEnd()
  $stderr = $process.StandardError.ReadToEnd()
  $process.WaitForExit()

  if ($process.ExitCode -ne 0) {
    throw "Command failed ($FilePath $($Arguments -join ' ')) with exit code $($process.ExitCode)`nSTDOUT:`n$stdout`nSTDERR:`n$stderr"
  }

  $combined = "$stdout`n$stderr"
  if ($Expected -and -not $combined.Contains($Expected)) {
    throw "Command output did not contain '$Expected' ($FilePath $($Arguments -join ' '))`nSTDOUT:`n$stdout`nSTDERR:`n$stderr"
  }

  return $stdout
}

function Copy-DirectoryContents {
  param(
    [Parameter(Mandatory = $true)][string]$Source,
    [Parameter(Mandatory = $true)][string]$Destination
  )

  New-Item -ItemType Directory -Force -Path $Destination | Out-Null
  Copy-Item -Path (Join-Path $Source "*") -Destination $Destination -Recurse -Force
}

function Get-FreeTcpPort {
  $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
  try {
    $listener.Start()
    return $listener.LocalEndpoint.Port
  } finally {
    $listener.Stop()
  }
}

function Start-VibeForegroundForSmokeTest {
  param(
    [Parameter(Mandatory = $true)][string]$FilePath,
    [Parameter(Mandatory = $true)][int]$Port
  )

  $psi = [System.Diagnostics.ProcessStartInfo]::new()
  $psi.FileName = $FilePath
  foreach ($arg in @("start", "--foreground", "--port", $Port.ToString())) {
    [void]$psi.ArgumentList.Add($arg)
  }
  $psi.WorkingDirectory = $RepoRoot
  $psi.RedirectStandardOutput = $true
  $psi.RedirectStandardError = $true
  $psi.UseShellExecute = $false

  $process = [System.Diagnostics.Process]::Start($psi)
  $healthUrl = "http://127.0.0.1:$Port/health"
  $deadline = [DateTime]::UtcNow.AddSeconds(30)
  do {
    if ($process.HasExited) {
      $stdout = $process.StandardOutput.ReadToEnd()
      $stderr = $process.StandardError.ReadToEnd()
      throw "vibe start exited before becoming healthy with exit code $($process.ExitCode)`nSTDOUT:`n$stdout`nSTDERR:`n$stderr"
    }

    try {
      $response = Invoke-WebRequest -UseBasicParsing -Uri $healthUrl -TimeoutSec 2
      if ($response.StatusCode -eq 200) {
        return $process
      }
    } catch {
      Start-Sleep -Milliseconds 500
    }
  } while ([DateTime]::UtcNow -lt $deadline)

  try {
    Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    $process.WaitForExit(5000) | Out-Null
  } catch {}
  throw "vibe start did not become healthy at $healthUrl within 30 seconds"
}

if (-not $IsWindows) {
  throw "This smoke test must run on Windows."
}

$testHome = Join-Path ([System.IO.Path]::GetTempPath()) ("vibe-home-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $testHome | Out-Null
$env:VIBE_HOME = $testHome

$ExePath = (Resolve-Path $ExePath).Path
Write-Host "Testing Windows CLI binary: $ExePath"

Invoke-Vibe -FilePath $ExePath -Arguments @("--help") -Expected "local API gateway" | Out-Null
Invoke-Vibe -FilePath $ExePath -Arguments @("--version") -Expected "vibe" | Out-Null
Invoke-Vibe -FilePath $ExePath -Arguments @("status") -Expected "vibe is not running" | Out-Null
Invoke-Vibe -FilePath $ExePath -Arguments @("config", "path") -Expected "vibe" | Out-Null
Invoke-Vibe -FilePath $ExePath -Arguments @("autostart", "status") -Expected "unsupported" | Out-Null

$smokePort = Get-FreeTcpPort
$vibeServer = $null
try {
  $vibeServer = Start-VibeForegroundForSmokeTest -FilePath $ExePath -Port $smokePort
  Invoke-Vibe -FilePath $ExePath -Arguments @("status") -Expected "endpoint:         http://127.0.0.1:$smokePort" | Out-Null
} finally {
  if ($null -ne $vibeServer -and -not $vibeServer.HasExited) {
    Stop-Process -Id $vibeServer.Id -Force -ErrorAction SilentlyContinue
    $vibeServer.WaitForExit(5000) | Out-Null
  }
}

$statusLineInput = '{"model":{"id":"claude-sonnet-4-5-20251001"},"context_window":{"current_usage":{"input_tokens":1000,"output_tokens":2}}}'
Invoke-Vibe -FilePath $ExePath -Arguments @("statusline") -Stdin $statusLineInput -Expected "Vibe+" | Out-Null

$workDir = Join-Path ([System.IO.Path]::GetTempPath()) ("vibe-windows-cli-" + [System.Guid]::NewGuid().ToString("N"))
$wrapperDir = Join-Path $workDir "cli"
$platformDir = Join-Path $workDir "cli-win32-x64"
$installDir = Join-Path $workDir "install"
try {
  New-Item -ItemType Directory -Force -Path $workDir, $wrapperDir, $platformDir, $installDir | Out-Null

  Copy-DirectoryContents -Source (Join-Path $RepoRoot "packages/cli-npm") -Destination $wrapperDir
  Remove-Item -Recurse -Force -ErrorAction SilentlyContinue (Join-Path $wrapperDir "platform")
  Remove-Item -Recurse -Force -ErrorAction SilentlyContinue (Join-Path $wrapperDir "scripts")

  Copy-Item -Path (Join-Path $RepoRoot "packages/cli-npm/platform/win32-x64/package.json") -Destination (Join-Path $platformDir "package.json") -Force
  New-Item -ItemType Directory -Force -Path (Join-Path $platformDir "bin") | Out-Null
  Copy-Item -Path $ExePath -Destination (Join-Path $platformDir "bin/vibe.exe") -Force

  Push-Location $workDir
  try {
    $platformPackJson = npm pack $platformDir --json | ConvertFrom-Json
    $wrapperPackJson = npm pack $wrapperDir --json | ConvertFrom-Json
    $platformTarball = Join-Path $workDir $platformPackJson[0].filename
    $wrapperTarball = Join-Path $workDir $wrapperPackJson[0].filename
  } finally {
    Pop-Location
  }

  Push-Location $installDir
  try {
    Set-Content -Path (Join-Path $installDir "package.json") -Value '{"private":true,"type":"module"}'
    npm install $platformTarball $wrapperTarball --include=optional --no-audit --no-fund --ignore-scripts | Out-Null

    $installedPlatform = Join-Path $installDir "node_modules/@vibe-plus/cli-win32-x64/bin/vibe.exe"
    if (-not (Test-Path $installedPlatform)) {
      throw "npm install did not include @vibe-plus/cli-win32-x64/bin/vibe.exe"
    }

    $installedVibeShim = Join-Path $installDir "node_modules/.bin/vibe.cmd"
    if (-not (Test-Path $installedVibeShim)) {
      throw "npm did not create the vibe.cmd binary shim"
    }

    Invoke-Vibe -FilePath $installedVibeShim -Arguments @("--help") -Expected "local API gateway" | Out-Null
    Invoke-Vibe -FilePath $installedVibeShim -Arguments @("--version") -Expected "vibe" | Out-Null
    Invoke-Vibe -FilePath $installedVibeShim -Arguments @("statusline") -Stdin $statusLineInput -Expected "Vibe+" | Out-Null
  } finally {
    Pop-Location
  }
} finally {
  Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $workDir
}

Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $testHome
Write-Host "Windows CLI smoke test passed."
