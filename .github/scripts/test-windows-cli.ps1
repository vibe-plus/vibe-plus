param(
  [string]$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path,
  [string]$ExePath = (Join-Path $RepoRoot "target/release/vibe.exe")
)

$ErrorActionPreference = "Stop"

if (-not $IsWindows) {
  throw "This smoke test must run on Windows."
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

$testHome = Join-Path ([System.IO.Path]::GetTempPath()) ("vibe-smoke-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $testHome | Out-Null
$env:VIBE_HOME = $testHome

$ExePath = (Resolve-Path $ExePath).Path
$port = Get-FreeTcpPort
$healthUrl = "http://127.0.0.1:$port/health"

Write-Host "Smoke test: $ExePath (port $port)"

$psi = [System.Diagnostics.ProcessStartInfo]::new()
$psi.FileName = $ExePath
foreach ($arg in @("up", "--foreground", "--port", $port.ToString())) {
  [void]$psi.ArgumentList.Add($arg)
}
$psi.WorkingDirectory = $RepoRoot
$psi.UseShellExecute = $false

$gateway = [System.Diagnostics.Process]::Start($psi)

try {
  $deadline = [DateTime]::UtcNow.AddSeconds(30)
  do {
    if ($gateway.HasExited) {
      throw "vibe exited with code $($gateway.ExitCode) before $healthUrl became reachable"
    }

    try {
      $response = Invoke-WebRequest -UseBasicParsing -Uri $healthUrl -TimeoutSec 2
      if ($response.StatusCode -eq 200) {
        Write-Host "Gateway reachable at $healthUrl"
        break
      }
    } catch {
      Start-Sleep -Milliseconds 500
    }
  } while ([DateTime]::UtcNow -lt $deadline)

  if ([DateTime]::UtcNow -ge $deadline) {
    throw "Gateway not reachable at $healthUrl within 30 seconds"
  }
} finally {
  if (-not $gateway.HasExited) {
    Stop-Process -Id $gateway.Id -Force -ErrorAction SilentlyContinue
    $gateway.WaitForExit(5000) | Out-Null
  }
  Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $testHome
}

Write-Host "Windows CLI smoke test passed."
