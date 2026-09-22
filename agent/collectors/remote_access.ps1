$ErrorActionPreference = 'SilentlyContinue'

function Get-ConfValue($path, $key) {
    if (-not (Test-Path $path)) { return $null }
    $line = Get-Content $path -ErrorAction SilentlyContinue | Where-Object { $_ -match "^$key=(.+)$" } | Select-Object -First 1
    if ($line -match "^$key=(.+)$") { return $Matches[1].Trim() }
    return $null
}

# --- Tailscale ---
$tailscale = [ordered]@{
    installed         = $false
    version           = $null
    connected         = $false
    backend_state     = $null
    tailscale_ips     = @()
    dns_name          = $null
    service_running   = $false
    startup_automatic = $false
}

$tsPaths = @(
    "$env:ProgramFiles\Tailscale\tailscale.exe",
    "${env:ProgramFiles(x86)}\Tailscale\tailscale.exe"
)
$tsExe = $tsPaths | Where-Object { Test-Path $_ } | Select-Object -First 1

if ($tsExe) {
    $tailscale.installed = $true
    try {
        $verOut = & $tsExe version 2>$null
        if ($verOut) { $tailscale.version = ($verOut | Select-Object -First 1).ToString().Trim() }
    } catch {}

    try {
        $statusJson = & $tsExe status --json 2>$null | ConvertFrom-Json
        if ($statusJson) {
            $tailscale.backend_state = $statusJson.BackendState
            $tailscale.connected = ($statusJson.BackendState -eq 'Running')
            if ($statusJson.Self) {
                $tailscale.dns_name = $statusJson.Self.DNSName
                $tailscale.tailscale_ips = @($statusJson.Self.TailscaleIPs | Where-Object { $_ })
            }
            if ($statusJson.BackendState -eq 'Running' -and $tailscale.tailscale_ips.Count -eq 0) {
                $tailscale.connected = $true
            }
        }
    } catch {}

    $tsSvc = Get-Service -Name 'Tailscale' -ErrorAction SilentlyContinue
    if ($tsSvc) {
        $tailscale.service_running = ($tsSvc.Status -eq 'Running')
        $tailscale.startup_automatic = ($tsSvc.StartType -eq 'Automatic')
    } else {
        $tsSvcW = Get-CimInstance Win32_Service -Filter "Name='Tailscale'" -ErrorAction SilentlyContinue
        if ($tsSvcW) {
            $tailscale.service_running = ($tsSvcW.State -eq 'Running')
            $tailscale.startup_automatic = ($tsSvcW.StartMode -eq 'Auto')
        }
    }
}

# --- AnyDesk ---
$anydesk = [ordered]@{
    installed         = $false
    client_id         = $null
    service_running   = $false
    startup_automatic = $false
    version           = $null
}

$anydeskPaths = @(
    "$env:ProgramFiles\AnyDesk\AnyDesk.exe",
    "${env:ProgramFiles(x86)}\AnyDesk\AnyDesk.exe"
)
$anydeskExe = $anydeskPaths | Where-Object { Test-Path $_ } | Select-Object -First 1

if ($anydeskExe) {
    $anydesk.installed = $true
    $anydesk.version = (Get-Item $anydeskExe).VersionInfo.FileVersion
}

$anydeskId = Get-ConfValue "$env:PROGRAMDATA\AnyDesk\system.conf" 'ad.anynet.id'
if (-not $anydeskId) {
    $anydeskId = Get-ConfValue "$env:APPDATA\AnyDesk\user.conf" 'ad.anynet.id'
}
if (-not $anydeskId) {
    try {
        $reg = Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\AnyDesk' -ErrorAction SilentlyContinue
        if ($reg.ClientID) { $anydeskId = $reg.ClientID }
        elseif ($reg.InstallID) { $anydeskId = $reg.InstallID }
    } catch {}
}
if (-not $anydeskId) {
    try {
        $reg = Get-ItemProperty 'HKCU:\Software\AnyDesk' -ErrorAction SilentlyContinue
        if ($reg.ClientID) { $anydeskId = $reg.ClientID }
    } catch {}
}
$anydesk.client_id = $anydeskId

$anydeskSvc = Get-Service -Name 'AnyDesk' -ErrorAction SilentlyContinue
if ($anydeskSvc) {
    $anydesk.service_running = ($anydeskSvc.Status -eq 'Running')
    $anydesk.startup_automatic = ($anydeskSvc.StartType -eq 'Automatic')
}

# --- TeamViewer (bonus) ---
$teamviewer = [ordered]@{
    installed = $false
    client_id = $null
}
$tvReg = 'HKLM:\SOFTWARE\WOW6432Node\TeamViewer'
if (Test-Path $tvReg) {
    $teamviewer.installed = $true
    $tv = Get-ItemProperty $tvReg -ErrorAction SilentlyContinue
    if ($tv.ClientID) { $teamviewer.client_id = $tv.ClientID }
}

$result = [ordered]@{
    tailscale   = $tailscale
    anydesk     = $anydesk
    teamviewer  = $teamviewer
}

$result | ConvertTo-Json -Compress -Depth 6
