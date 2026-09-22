# Belarc Inventory Agent - Instalador (servico Windows, segundo plano)
param(
    [Parameter(Mandatory = $true)]
    [string]$ServerUrl,

    [Parameter(Mandatory = $true)]
    [string]$AgentToken,

    [string]$InstallDir = "$env:ProgramFiles\BelarcInventory"
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path ".\belarc-agent.exe")) {
    Write-Error "belarc-agent.exe not found in current directory."
}

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Copy-Item ".\belarc-agent.exe" "$InstallDir\belarc-agent.exe" -Force
if (Test-Path ".\collectors") {
    Copy-Item ".\collectors" "$InstallDir\collectors" -Recurse -Force
} elseif (Test-Path "..\agent\collectors") {
    Copy-Item "..\agent\collectors" "$InstallDir\collectors" -Recurse -Force
}

$configDir = "$env:ProgramData\BelarcInventory"
New-Item -ItemType Directory -Force -Path $configDir | Out-Null

@"
server_url = "$ServerUrl"
agent_token = "$AgentToken"
heartbeat_interval_seconds = 180
t1_interval_seconds = 21600
t2_interval_seconds = 86400
"@ | Set-Content "$configDir\config.toml" -Encoding UTF8

# Atualizar binPath se servico ja existir
$svc = Get-Service -Name 'BelarcInventoryAgent' -ErrorAction SilentlyContinue
if ($svc) {
    sc.exe stop BelarcInventoryAgent 2>$null | Out-Null
    Start-Sleep -Seconds 2
    sc.exe delete BelarcInventoryAgent 2>$null | Out-Null
    Start-Sleep -Seconds 1
}

& "$InstallDir\belarc-agent.exe" install

Write-Host "Agent installed to $InstallDir"
Write-Host "Server: $ServerUrl"
Write-Host "Service: BelarcInventoryAgent (auto-start)"
