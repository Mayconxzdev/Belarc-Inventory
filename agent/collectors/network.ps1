$ErrorActionPreference = 'SilentlyContinue'

$adapters = @(Get-CimInstance Win32_NetworkAdapterConfiguration | Where-Object { $_.IPEnabled -eq $true } | ForEach-Object {
    [ordered]@{
        description = $_.Description
        mac         = $_.MACAddress
        ips         = @($_.IPAddress | Where-Object { $_ -match '^\d' })
        dhcp        = $_.DHCPEnabled
        dns         = @($_.DNSServerSearchOrder)
        gateway     = @($_.DefaultIPGateway)
    }
})

$smbMappings = @()
try {
    $smbMappings = @(Get-SmbMapping -ErrorAction SilentlyContinue | ForEach-Object {
        [ordered]@{ local = $_.LocalPath; remote = $_.RemotePath; status = $_.Status }
    })
} catch {}

$psDrives = @(Get-PSDrive -PSProvider FileSystem -ErrorAction SilentlyContinue | Where-Object { $_.DisplayRoot } | ForEach-Object {
    [ordered]@{ name = $_.Name; root = $_.Root; display_root = $_.DisplayRoot }
})

$wifiProfiles = @()
try {
    $profiles = netsh wlan show profiles 2>$null | Select-String 'All User Profile\s*:\s*(.+)' | ForEach-Object { $_.Matches.Groups[1].Value.Trim() }
    $wifiProfiles = @($profiles | ForEach-Object { @{ ssid = $_ } })
} catch {}

$firewallProfiles = @()
try {
    $firewallProfiles = @(Get-NetFirewallProfile -ErrorAction SilentlyContinue | ForEach-Object {
        [ordered]@{ name = $_.Name; enabled = $_.Enabled }
    })
} catch {}

$shares = @(Get-CimInstance Win32_Share | Where-Object { $_.Type -eq 0 } | ForEach-Object {
    [ordered]@{ name = $_.Name; path = $_.Path }
})

$lanIps = @()
$primaryLanIp = $null
foreach ($a in $adapters) {
    foreach ($ip in $a.ips) {
        if ($ip -match '^(10\.|172\.(1[6-9]|2\d|3[01])\.|192\.168\.)') {
            $lanIps += $ip
            if (-not $primaryLanIp) { $primaryLanIp = $ip }
        } elseif ($ip -notmatch '^127\.' -and -not $primaryLanIp) {
            $primaryLanIp = $ip
            $lanIps += $ip
        }
    }
}
if (-not $primaryLanIp -and $adapters.Count -gt 0 -and $adapters[0].ips.Count -gt 0) {
    $primaryLanIp = $adapters[0].ips[0]
}

$result = [ordered]@{
    primary_lan_ip = $primaryLanIp
    lan_ips        = @($lanIps | Select-Object -Unique)
    adapters       = $adapters
    smb_mappings   = $smbMappings
    network_drives = $psDrives
    wifi_profiles  = $wifiProfiles
    firewall       = $firewallProfiles
    local_shares   = $shares
}

$result | ConvertTo-Json -Compress -Depth 6
