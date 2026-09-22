$ErrorActionPreference = 'SilentlyContinue'

$RootIssuers = @(
    'MICROSOFT', 'BALTIMORE', 'VERISIGN', 'THAWTE', 'DIGICERT', 'GLOBALSIGN',
    'COMODO', 'ENTRUST', 'GEOTRUST', 'GODADDY', 'STARFIELD', 'USERTRUST', 'DST'
)

function Get-CertClass($cert, $store) {
    $subject = ($cert.Subject + '').ToUpper()
    $issuer  = ($cert.Issuer + '').ToUpper()
    $hasPk   = $cert.HasPrivateKey
    $cn = if ($subject -match 'CN=([^,]+)') { $Matches[1].Trim().ToUpper() } else { '' }

    if ($store -match '\\Root$' -and -not $hasPk) {
        foreach ($r in $RootIssuers) {
            if ($issuer -match $r) { return 'root_historical' }
        }
        return 'root_historical'
    }
    if ($hasPk -and (
        $subject -match 'ICP-BRASIL' -or $subject -match 'E-CNPJ' -or $subject -match 'RFB' -or
        $subject -match 'CN=[^:]+:\d{11,}'
    )) { return 'corporate' }
    if ($hasPk -and (
        $issuer -eq $subject -or $cn -match 'PROJETO' -or $cn -match 'DESKTOP' -or $cn -match 'WIN-'
    )) { return 'self_signed' }
    if ($store -match '\\My$') { return 'other' }
    'root_historical'
}

function Get-CertInfo($cert, $store) {
    $daysLeft = ($cert.NotAfter - (Get-Date)).Days
    [ordered]@{
        subject     = $cert.Subject
        issuer      = $cert.Issuer
        thumbprint  = $cert.Thumbprint
        not_before  = $cert.NotBefore.ToString('o')
        not_after   = $cert.NotAfter.ToString('o')
        days_left   = $daysLeft
        has_private_key = $cert.HasPrivateKey
        cert_class  = Get-CertClass $cert $store
        store       = $store
    }
}

$stores = @(
    'Cert:\LocalMachine\My',
    'Cert:\LocalMachine\Root',
    'Cert:\CurrentUser\My'
)

$all = @()
$expiring = @()
$expired = @()

foreach ($store in $stores) {
    if (Test-Path $store) {
        Get-ChildItem $store -ErrorAction SilentlyContinue | ForEach-Object {
            $info = Get-CertInfo $_ $store
            $all += $info
            if ($info.days_left -lt 0) { $expired += $info }
            elseif ($info.days_left -le 30) { $expiring += $info }
        }
    }
}

$result = [ordered]@{
    total_count   = $all.Count
    certificates  = @($all | Select-Object -First 200)
    expiring_soon = $expiring
    expired       = $expired
}

$result | ConvertTo-Json -Compress -Depth 6
