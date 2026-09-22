$ErrorActionPreference = 'SilentlyContinue'

$os = Get-CimInstance Win32_OperatingSystem
$lic = Get-CimInstance SoftwareLicensingService -ErrorAction SilentlyContinue

$activation = 'Unknown'
$key_partial = $null
try {
    $status = (Get-CimInstance SoftwareLicensingProduct -Filter "ApplicationID='55c92734-d682-4d71-983e-d6ec3f16059f' AND LicenseStatus=1" -ErrorAction SilentlyContinue | Select-Object -First 1)
    if ($status) { $activation = 'Licensed' } else { $activation = 'NotLicensed' }
} catch {}

try {
    if ($lic.OA3xOriginalProductKey) {
        $full = $lic.OA3xOriginalProductKey
        if ($full.Length -ge 5) { $key_partial = $full.Substring($full.Length - 5) }
    }
} catch {}

$hotfixes = @(Get-CimInstance Win32_QuickFixEngineering | Select-Object HotFixID, InstalledOn, Description)

$features = @()
@('.NET Framework', 'Hyper-V', 'WSL') | ForEach-Object {
    $features += @{ name = $_; installed = $false }
}

$rdp = (Get-ItemProperty 'HKLM:\System\CurrentControlSet\Control\Terminal Server' -Name fDenyTSConnections -ErrorAction SilentlyContinue).fDenyTSConnections

$result = [ordered]@{
    caption           = $os.Caption
    version           = $os.Version
    build             = $os.BuildNumber
    architecture      = $os.OSArchitecture
    install_date      = $os.InstallDate
    last_boot         = $os.LastBootUpTime.ToString('o')
    activation_status = $activation
    product_key_partial = $key_partial
    rdp_enabled       = ($rdp -eq 0)
    hotfixes          = @($hotfixes | ForEach-Object { @{ id = $_.HotFixID; installed_on = $_.InstalledOn; description = $_.Description } })
    hotfix_count      = $hotfixes.Count
}

$result | ConvertTo-Json -Compress -Depth 6
