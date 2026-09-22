$ErrorActionPreference = 'SilentlyContinue'

function Format-ProductKey($key) {
    if (-not $key) { return $null }
    $k = ($key -replace '[^A-Za-z0-9]', '').ToUpper()
    if ($k.Length -lt 20) { return $key }
    ($k -split '(.{5})' | Where-Object { $_ }) -join '-'
}

$windows = [ordered]@{
    edition             = (Get-CimInstance Win32_OperatingSystem).Caption
    activation          = 'Unknown'
    product_key         = $null
    product_key_partial = $null
    license_channel     = $null
    digital_license     = $null
}

try {
    $lic = Get-CimInstance SoftwareLicensingService -ErrorAction SilentlyContinue
    if ($lic) {
        if ($lic.OA3xOriginalProductKey) {
            $windows.product_key = Format-ProductKey $lic.OA3xOriginalProductKey
        }
        $windows.digital_license = $lic.DigitalLicense
    }

    $products = Get-CimInstance SoftwareLicensingProduct -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -like '*Windows*' -or $_.ApplicationID -eq '55c92734-d682-4d71-983e-d6ec3f16059f' }

    foreach ($prod in $products) {
        $windows.activation = switch ($prod.LicenseStatus) {
            1 { 'Licensed' }
            0 { 'Unlicensed' }
            default { "Status$($prod.LicenseStatus)" }
        }
        if ($prod.PartialProductKey) {
            $windows.product_key_partial = $prod.PartialProductKey
        }
        if ($prod.Description) { $windows.license_channel = $prod.Description }
        if ($prod.LicenseIsAddon) { $windows.is_addon = $prod.LicenseIsAddon }
    }
} catch {}

try {
    $slmgr = cscript //nologo "$env:SystemRoot\System32\slmgr.vbs" /dli 2>$null
    if ($slmgr) {
        $windows.slmgr_info = @($slmgr | Where-Object { $_.Trim() } | Select-Object -First 15)
        foreach ($line in $slmgr) {
            if ($line -match 'Partial Product Key:\s*(\S+)') {
                $windows.product_key_partial = $Matches[1]
            }
        }
    }
} catch {}

# Office - OSPP (Volume/Retail) + Click-to-Run registry
$office = @()
$osppPaths = @()
@("${env:ProgramFiles}\Microsoft Office", "${env:ProgramFiles(x86)}\Microsoft Office") | ForEach-Object {
    if (Test-Path $_) {
        $osppPaths += Get-ChildItem -Path $_ -Recurse -Filter 'OSPP.VBS' -ErrorAction SilentlyContinue |
            Select-Object -ExpandProperty FullName -Unique
    }
}

foreach ($ospp in $osppPaths) {
    try {
        $out = & cscript //nologo $ospp '/dstatus' 2>$null
        $current = $null
        foreach ($line in ($out -split "`n")) {
            if ($line -match 'PRODUCT ID:\s*(.+)') {
                if ($current) { $office += $current }
                $current = [ordered]@{ product_id = $Matches[1].Trim(); source = 'OSPP' }
            }
            elseif ($line -match 'LICENSE NAME:\s*(.+)' -and $current) { $current.license_name = $Matches[1].Trim() }
            elseif ($line -match 'LICENSE STATUS:\s*(.+)' -and $current) { $current.license_status = $Matches[1].Trim() }
            elseif ($line -match 'Last 5 characters of installed product key:\s*(.+)' -and $current) {
                $current.key_partial = $Matches[1].Trim()
            }
            elseif ($line -match 'Activation Type Configuration:\s*(.+)' -and $current) {
                $current.activation_type = $Matches[1].Trim()
            }
        }
        if ($current) { $office += $current }
    } catch {}
}

# Office Click-to-Run / M365
$c2rPaths = @(
    'HKLM:\SOFTWARE\Microsoft\Office\ClickToRun\Configuration',
    'HKLM:\SOFTWARE\Microsoft\Office\16.0\Common\Licensing'
)
foreach ($regPath in $c2rPaths) {
    if (Test-Path $regPath) {
        $props = Get-ItemProperty $regPath -ErrorAction SilentlyContinue
        if ($props) {
            $office += [ordered]@{
                source         = 'ClickToRun'
                product_id     = $props.ProductReleaseIds
                license_status = $props.LicenseStatus
                version        = $props.VersionToReport
                audience       = $props.AudienceId
            }
        }
    }
}

# Office registry ProductKeys (quando disponivel)
$officeReg = 'HKLM:\SOFTWARE\Microsoft\Office'
if (Test-Path $officeReg) {
    Get-ChildItem $officeReg -ErrorAction SilentlyContinue | ForEach-Object {
        $ver = $_.PSChildName
        $licPath = Join-Path $_.PSPath 'Registration'
        if (Test-Path $licPath) {
            Get-ChildItem $licPath -ErrorAction SilentlyContinue | ForEach-Object {
                $p = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue
                if ($p.DigitalProductId -or $p.ProductID) {
                    $office += [ordered]@{
                        source     = 'Registry'
                        version    = $ver
                        product_id = $p.ProductID
                    }
                }
            }
        }
    }
}

$adobe = @()
if (Test-Path 'HKLM:\SOFTWARE\Adobe') {
    Get-ChildItem 'HKLM:\SOFTWARE\Adobe' -ErrorAction SilentlyContinue | ForEach-Object {
        $adobe += [ordered]@{ product = $_.PSChildName }
    }
}

$result = [ordered]@{
    windows = $windows
    office  = @($office | Select-Object -Unique)
    adobe   = $adobe
    note    = 'Chaves exibidas conforme expostas pelo sistema. M365/Adobe subscription pode nao ter chave retail.'
}

$result | ConvertTo-Json -Compress -Depth 8
