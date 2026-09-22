$ErrorActionPreference = 'SilentlyContinue'

$defender = $null
try {
    $mp = Get-MpComputerStatus -ErrorAction SilentlyContinue
    if ($mp) {
        $defender = [ordered]@{
            enabled                 = $mp.AntivirusEnabled
            real_time_protection  = $mp.RealTimeProtectionEnabled
            definitions_up_to_date  = $mp.AntivirusSignatureLastUpdated -gt (Get-Date).AddDays(-3)
            definitions_date        = $mp.AntivirusSignatureLastUpdated.ToString('o')
            last_quick_scan         = $mp.QuickScanEndTime.ToString('o')
        }
    }
} catch {}

function Get-AvProductName($product) {
    $name = $product.displayName
    if ($name -and $name.Trim().Length -gt 2 -and $name -notmatch '^\s*[\x00-\x1F]') {
        return $name.Trim()
    }
    $path = $product.pathToSignedProductExe
    if ($path) {
        $leaf = Split-Path (Split-Path $path -Parent) -Leaf
        if ($leaf) { return $leaf }
    }
    return $null
}

function Get-AvProductState($state) {
    $enabled = ($state -band 0x1000) -ne 0
    $upToDate = ($state -band 0x10) -ne 0
    [ordered]@{ enabled = $enabled; up_to_date = $upToDate; raw = $state }
}

$avProducts = @()
try {
    $avProducts = @(Get-CimInstance -Namespace root\SecurityCenter2 -ClassName AntiVirusProduct -ErrorAction SilentlyContinue | ForEach-Object {
        $parsed = Get-AvProductState $_.productState
        [ordered]@{
            display_name = Get-AvProductName $_
            path         = $_.pathToSignedProductExe
            enabled      = $parsed.enabled
            up_to_date   = $parsed.up_to_date
            product_state = $_.productState
        }
    })
} catch {}

$eset = [ordered]@{
    installed        = $false
    product_name     = $null
    version          = $null
    agent_version    = $null
    install_path     = $null
    service_running  = $false
    real_time_active = $null
}

$esetPaths = @(
    'C:\Program Files\ESET\ESET Security\ekrn.exe',
    'C:\Program Files (x86)\ESET\ESET Security\ekrn.exe'
)
$esetExe = $esetPaths | Where-Object { Test-Path $_ } | Select-Object -First 1
if ($esetExe) {
    $eset.installed = $true
    $eset.install_path = Split-Path $esetExe -Parent
    try { $eset.version = (Get-Item $esetExe).VersionInfo.ProductVersion } catch {}
    $svc = Get-CimInstance Win32_Service -Filter "Name='ekrn'" -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($svc) { $eset.service_running = ($svc.State -eq 'Running') }
}

foreach ($av in $avProducts) {
    if ($av.display_name -match 'ESET') {
        $eset.installed = $true
        $eset.product_name = $av.display_name
        $eset.real_time_active = $av.enabled
        break
    }
}

$esetReg = Get-ItemProperty 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*',
    'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*' -ErrorAction SilentlyContinue |
    Where-Object { $_.DisplayName -match '^ESET Endpoint Security$|^ESET Security$' } | Select-Object -First 1
if ($esetReg) {
    $eset.installed = $true
    $eset.product_name = $esetReg.DisplayName
    $eset.version = $esetReg.DisplayVersion
    if ($esetReg.InstallLocation) { $eset.install_path = $esetReg.InstallLocation }
}

$esetAgent = Get-ItemProperty 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*',
    'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*' -ErrorAction SilentlyContinue |
    Where-Object { $_.DisplayName -match 'ESET Management Agent' } | Select-Object -First 1
if ($esetAgent) { $eset.agent_version = $esetAgent.DisplayVersion }

$firewall = [ordered]@{ enabled = $true; profiles = @() }
try {
    $profiles = Get-NetFirewallProfile -ErrorAction SilentlyContinue
    $firewall.profiles = @($profiles | ForEach-Object { [ordered]@{ name = $_.Name; enabled = $_.Enabled } })
    $firewall.enabled = -not ($profiles | Where-Object { -not $_.Enabled })
} catch {}

$bitlocker = @()
try {
    $bitlocker = @(Get-BitLockerVolume -ErrorAction SilentlyContinue | ForEach-Object {
        [ordered]@{
            mount_point      = $_.MountPoint
            protection_status = $_.ProtectionStatus.ToString()
            encryption_percent = $_.EncryptionPercentage
        }
    })
} catch {}

$localAdmins = @()
try {
    $localAdmins = @(Get-LocalGroupMember -Group 'Administrators' -ErrorAction SilentlyContinue | ForEach-Object {
        [ordered]@{ name = $_.Name; object_class = $_.ObjectClass; principal_source = $_.PrincipalSource.ToString() }
    })
} catch {}

$localUsers = @(Get-CimInstance Win32_UserAccount -Filter "LocalAccount=True" | ForEach-Object {
    [ordered]@{
        name      = $_.Name
        disabled  = $_.Disabled
        sid       = $_.SID
    }
})

$result = [ordered]@{
    defender      = $defender
    av_products   = $avProducts
    eset          = $eset
    firewall      = $firewall
    bitlocker     = $bitlocker
    local_admins  = $localAdmins
    local_users   = $localUsers
}

$result | ConvertTo-Json -Compress -Depth 6
