$ErrorActionPreference = 'SilentlyContinue'

. (Join-Path $PSScriptRoot '_user-context.ps1')

$cs = Get-CimInstance Win32_ComputerSystem
$os = Get-CimInstance Win32_OperatingSystem
$bios = Get-CimInstance Win32_BIOS
$product = Get-CimInstance Win32_ComputerSystemProduct
$nic = Get-CimInstance Win32_NetworkAdapterConfiguration | Where-Object { $_.IPEnabled -eq $true } | Select-Object -First 1
$enclosure = Get-CimInstance Win32_SystemEnclosure

try { $fqdn = [System.Net.Dns]::GetHostEntry($env:COMPUTERNAME).HostName } catch { $fqdn = $env:COMPUTERNAME }

$uptime = if ($os -and $os.LastBootUpTime) { (Get-Date) - $os.LastBootUpTime } else { [TimeSpan]::Zero }

$loggedUsers = @(Get-BelarcInteractiveUsers)
$loggedUser = if ($loggedUsers.Count -gt 0) { $loggedUsers[0] } else { $cs.UserName }

$result = [ordered]@{
    hostname      = $env:COMPUTERNAME
    fqdn          = $fqdn
    domain        = if ($cs) { $cs.Domain } else { $null }
    workgroup     = if ($cs) { $cs.Workgroup } else { $null }
    manufacturer  = if ($cs) { $cs.Manufacturer } else { $null }
    model         = if ($cs) { $cs.Model } else { $null }
    system_type   = if ($cs) { $cs.SystemFamily } else { $null }
    chassis       = if ($enclosure -and $enclosure.ChassisTypes) { $enclosure.ChassisTypes[0] } else { $null }
    serial        = if ($bios) { $bios.SerialNumber } else { $null }
    machine_uuid  = if ($product) { $product.UUID } else { $null }
    logged_user   = $loggedUser
    logged_users  = $loggedUsers
    uptime_seconds = [int]$uptime.TotalSeconds
    last_boot     = if ($os -and $os.LastBootUpTime) { $os.LastBootUpTime.ToString('o') } else { $null }
    timezone      = [System.TimeZoneInfo]::Local.Id
    locale        = (Get-Culture).Name
    mac_primary   = if ($nic) { $nic.MACAddress } else { $null }
    ip_primary    = if ($nic) { ($nic.IPAddress | Where-Object { $_ -match '^\d' } | Select-Object -First 1) } else { $null }
}

$result | ConvertTo-Json -Compress -Depth 5
