# Helpers para coletores rodando como servico Windows (LOCAL SYSTEM).
function Get-BelarcInteractiveUsers {
    $users = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)

    try {
        $cs = Get-CimInstance Win32_ComputerSystem -ErrorAction Stop
        if ($cs.UserName) { [void]$users.Add($cs.UserName) }
    } catch { }

    try {
        Get-CimInstance Win32_LoggedOnUser -ErrorAction Stop | ForEach-Object {
            $part = $_.Antecedent.ToString()
            if ($part -match 'Domain="([^"]*)",Name="([^"]+)"') {
                $domain = $Matches[1]
                $name = $Matches[2]
                if ($name -and $name -notmatch '^(DWM-|UMFD-|SYSTEM|LOCAL SERVICE|NETWORK SERVICE)$') {
                    if ($domain -and $domain -ne '.' -and $domain -ne $env:COMPUTERNAME) {
                        [void]$users.Add("$domain\$name")
                    } else {
                        [void]$users.Add($name)
                    }
                }
            }
        }
    } catch { }

    try {
        $q = quser 2>$null
        if ($q) {
            $q | Select-Object -Skip 1 | ForEach-Object {
                $line = ($_ -replace '\s{2,}', '|').Trim('|')
                $parts = $line -split '\|'
                if ($parts.Count -ge 1 -and $parts[0] -match '^\S+$' -and $parts[0] -ne 'USERNAME') {
                    [void]$users.Add($parts[0].Trim('>'))
                }
            }
        }
    } catch { }

    @($users)
}

function Get-BelarcUserProfiles {
    $profiles = @()
    $seen = @{}

    foreach ($user in Get-BelarcInteractiveUsers) {
        $sam = ($user -split '\\')[-1]
        try {
            $nt = New-Object System.Security.Principal.NTAccount($user)
            $sid = $nt.Translate([System.Security.Principal.SecurityIdentifier]).Value
            $regPath = "Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList\$sid"
            if (-not (Test-Path $regPath)) { continue }
            $profilePath = (Get-ItemProperty $regPath -ErrorAction Stop).ProfileImagePath
            if (-not $profilePath -or -not (Test-Path $profilePath)) { continue }
            if ($seen.ContainsKey($sid)) { continue }
            $seen[$sid] = $true
            $profiles += [pscustomobject]@{
                user         = $user
                sid          = $sid
                profile_path = $profilePath
                appdata      = Join-Path $profilePath 'AppData\Roaming'
                appdata_local = Join-Path $profilePath 'AppData\Local'
                registry_root = "Registry::HKEY_USERS\$sid"
            }
        } catch { }
    }

    if ($profiles.Count -eq 0) {
        Get-ChildItem 'Registry::HKEY_USERS' -ErrorAction SilentlyContinue |
            Where-Object { $_.PSChildName -match '^S-1-5-21-' } |
            ForEach-Object {
                $sid = $_.PSChildName
                $regPath = "Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList\$sid"
                if (-not (Test-Path $regPath)) { return }
                $profilePath = (Get-ItemProperty $regPath -ErrorAction SilentlyContinue).ProfileImagePath
                if (-not $profilePath -or -not (Test-Path $profilePath)) { return }
                if ($seen.ContainsKey($sid)) { return }
                $seen[$sid] = $true
                $profiles += [pscustomobject]@{
                    user         = $sid
                    sid          = $sid
                    profile_path = $profilePath
                    appdata      = Join-Path $profilePath 'AppData\Roaming'
                    appdata_local = Join-Path $profilePath 'AppData\Local'
                    registry_root = "Registry::HKEY_USERS\$sid"
                }
            }
    }

    @($profiles)
}

function Get-BelarcUserRegistryUninstallPaths {
    $paths = @('HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*')
    foreach ($profile in Get-BelarcUserProfiles) {
        $paths += "$($profile.registry_root)\Software\Microsoft\Windows\CurrentVersion\Uninstall\*"
        $paths += "$($profile.registry_root)\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*"
    }
    @($paths | Select-Object -Unique)
}

function Get-BelarcUserRunKeys {
    $keys = @('HKCU:\Software\Microsoft\Windows\CurrentVersion\Run')
    foreach ($profile in Get-BelarcUserProfiles) {
        $keys += "$($profile.registry_root)\Software\Microsoft\Windows\CurrentVersion\Run"
    }
    @($keys | Select-Object -Unique)
}
