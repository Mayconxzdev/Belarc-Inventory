$ErrorActionPreference = 'SilentlyContinue'

$groups = @()
try {
    $groups = @(Get-LocalGroup -ErrorAction SilentlyContinue | ForEach-Object {
        $members = @()
        try {
            $members = @(Get-LocalGroupMember -Group $_.Name -ErrorAction SilentlyContinue | ForEach-Object { $_.Name })
        } catch {}
        [ordered]@{
            name    = $_.Name
            sid     = $_.SID.Value
            members = $members
        }
    })
} catch {}

$gpo = $null
try {
    $gpoResult = gpresult /r /scope computer 2>$null
    if ($gpoResult) {
        $gpo = @{ raw = ($gpoResult -join "`n").Substring(0, [Math]::Min(2000, ($gpoResult -join "`n").Length)) }
    }
} catch {}

$aclPaths = @('C:\Users', 'C:\ProgramData')
if ($env:BELARC_ACL_PATHS) {
    $aclPaths = $env:BELARC_ACL_PATHS -split ';'
}

$acls = @()
foreach ($path in $aclPaths) {
    if (Test-Path $path) {
        try {
            $acl = Get-Acl $path -ErrorAction SilentlyContinue
            $access = @($acl.Access | Select-Object -First 20 | ForEach-Object {
                [ordered]@{
                    identity = $_.IdentityReference.Value
                    rights   = $_.FileSystemRights.ToString()
                    type     = $_.AccessControlType.ToString()
                }
            })
            $acls += [ordered]@{ path = $path; owner = $acl.Owner; access = $access }
        } catch {}
    }
}

$result = [ordered]@{
    local_groups = $groups
    gpo_summary  = $gpo
    acls         = $acls
}

$result | ConvertTo-Json -Compress -Depth 8
