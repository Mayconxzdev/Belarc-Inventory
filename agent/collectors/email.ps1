$ErrorActionPreference = 'SilentlyContinue'

. (Join-Path $PSScriptRoot '_user-context.ps1')

function Get-EmailFromImapUrl($url) {
    if ($url -match 'imap://([^@]+)@') {
        $user = $Matches[1] -replace '%40', '@'
        if ($user -match '@') { return $user }
    }
    return $null
}

function Extract-EmailsFromPrefs($prefsPath) {
    $emails = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $accounts = @()
    if (-not (Test-Path $prefsPath)) { return @{ emails = @(); accounts = @() } }

    $content = Get-Content $prefsPath -Raw -ErrorAction SilentlyContinue
    if (-not $content) { return @{ emails = @(); accounts = @() } }

    # mail.identity.id1.useremail (formato correto - singular identity)
    [regex]::Matches($content, 'user_pref\("mail\.identity\.id\d+\.useremail",\s*"([^"]+)"\)') | ForEach-Object {
        [void]$emails.Add($_.Groups[1].Value)
    }

    # mail.server.serverN.userName quando parece e-mail
    [regex]::Matches($content, 'user_pref\("mail\.server\.server\d+\.userName",\s*"([^"]+)"\)') | ForEach-Object {
        $u = $_.Groups[1].Value
        if ($u -match '^[^@\s]+@[^@\s]+\.[^@\s]+$' -and $u -ne 'nobody') {
            [void]$emails.Add($u)
        }
    }

    # E-mails embutidos em URLs imap://usuario%40dominio@servidor
    [regex]::Matches($content, 'imap://[^"\s]+') | ForEach-Object {
        $e = Get-EmailFromImapUrl $_.Value
        if ($e) { [void]$emails.Add($e) }
    }

    # Contas configuradas (account -> server -> identity)
    $defaultAccount = $null
    if ($content -match 'user_pref\("mail\.accountmanager\.defaultaccount",\s*"([^"]+)"\)') {
        $defaultAccount = $Matches[1]
    }

    [regex]::Matches($content, 'user_pref\("mail\.account\.(account\d+)\.identities",\s*"([^"]+)"\)') | ForEach-Object {
        $accId = $_.Groups[1].Value
        $identityIds = $_.Groups[2].Value -split ','
        $accEmails = @()
        foreach ($idKey in $identityIds) {
            if ($content -match "user_pref\(`"mail\.identity\.$idKey\.useremail`",\s*`"([^`"]+)`"\)") {
                $accEmails += $Matches[1]
                [void]$emails.Add($Matches[1])
            }
        }
        $serverKey = $null
        if ($content -match "user_pref\(`"mail\.account\.$accId\.server`",\s*`"([^`"]+)`"\)") {
            $serverKey = $Matches[1]
        }
        $hostname = $null
        $type = $null
        if ($serverKey -and $content -match "user_pref\(`"mail\.server\.$serverKey\.hostname`",\s*`"([^`"]+)`"\)") {
            $hostname = $Matches[1]
        }
        if ($serverKey -and $content -match "user_pref\(`"mail\.server\.$serverKey\.type`",\s*`"([^`"]+)`"\)") {
            $type = $Matches[1]
        }
        $accounts += [ordered]@{
            account_id   = $accId
            is_default   = ($accId -eq $defaultAccount)
            emails       = @($accEmails | Select-Object -Unique)
            server       = $hostname
            server_type  = $type
        }
    }

    @{ emails = @($emails); accounts = $accounts }
}

function Parse-ThunderbirdProfiles($iniPath) {
    $baseDir = Split-Path $iniPath -Parent
    $profilesDir = Join-Path $baseDir 'Profiles'
    $result = @()
    $defaultProfilePath = $null

    $sections = @{}
    $current = $null
    foreach ($line in (Get-Content $iniPath -ErrorAction SilentlyContinue)) {
        $line = $line.Trim()
        if ($line -match '^\[(.+)\]$') {
            $current = $Matches[1]
            $sections[$current] = @{}
        }
        elseif ($current -and $line -match '^([^=]+)=(.*)$') {
            $sections[$current][$Matches[1]] = $Matches[2]
        }
    }

    foreach ($key in $sections.Keys) {
        if ($key -match '^Install' -and $sections[$key]['Default']) {
            $rel = $sections[$key]['Default']
            $defaultProfilePath = if ($rel -match '^Profiles/') {
                Join-Path $baseDir ($rel -replace '/', '\')
            } else { Join-Path $profilesDir $rel }
        }
    }

    foreach ($key in ($sections.Keys | Where-Object { $_ -match '^Profile\d+$' })) {
        $sec = $sections[$key]
        $relPath = $sec['Path']
        if (-not $relPath) { continue }

        $fullPath = if ($sec['IsRelative'] -eq '1') {
            if ($relPath -match '^Profiles/') {
                Join-Path $baseDir ($relPath -replace '/', '\')
            } else {
                Join-Path $profilesDir $relPath
            }
        } else {
            $relPath
        }

        $isDefault = ($sec['Default'] -eq '1') -or ($fullPath -eq $defaultProfilePath)

        $result += [ordered]@{
            profile_name = $sec['Name']
            profile_path = $fullPath
            is_default   = $isDefault
            exists       = (Test-Path $fullPath)
        }
    }

    if ($result.Count -eq 0 -and (Test-Path $profilesDir)) {
        Get-ChildItem $profilesDir -Directory -ErrorAction SilentlyContinue | ForEach-Object {
            $result += [ordered]@{
                profile_name = $_.Name
                profile_path = $_.FullName
                is_default   = $false
                exists       = $true
            }
        }
    }

    @($result)
}

# --- Thunderbird: perfis de cada usuario logado (funciona como servico SYSTEM) ---
$thunderbird = @()
$userProfiles = @(Get-BelarcUserProfiles)
if ($userProfiles.Count -eq 0) {
    $userProfiles = @([pscustomobject]@{ user = $env:USERNAME; appdata = $env:APPDATA })
}

foreach ($up in $userProfiles) {
    $tbBase = Join-Path $up.appdata 'Thunderbird'
    $tbIni = Join-Path $tbBase 'profiles.ini'

    if (Test-Path $tbIni) {
        $profileList = Parse-ThunderbirdProfiles $tbIni
        foreach ($prof in $profileList) {
            if (-not $prof.exists) { continue }
            $prefs = Join-Path $prof.profile_path 'prefs.js'
            $parsed = Extract-EmailsFromPrefs $prefs
            $thunderbird += [ordered]@{
                windows_user  = $up.user
                profile_name  = $prof.profile_name
                profile_path  = $prof.profile_path
                is_default    = $prof.is_default
                emails        = @($parsed.emails | Sort-Object)
                accounts      = $parsed.accounts
                email_count   = $parsed.emails.Count
            }
        }
    } elseif (Test-Path (Join-Path $tbBase 'Profiles')) {
        Get-ChildItem (Join-Path $tbBase 'Profiles') -Directory | ForEach-Object {
            $parsed = Extract-EmailsFromPrefs (Join-Path $_.FullName 'prefs.js')
            $thunderbird += [ordered]@{
                windows_user  = $up.user
                profile_name  = $_.Name
                profile_path  = $_.FullName
                is_default    = $false
                emails        = @($parsed.emails | Sort-Object)
                accounts      = $parsed.accounts
                email_count   = $parsed.emails.Count
            }
        }
    }
}

$tbBase = if ($userProfiles.Count -gt 0) { Join-Path $userProfiles[0].appdata 'Thunderbird' } else { Join-Path $env:APPDATA 'Thunderbird' }

$tbSummary = [ordered]@{
    installed       = ($thunderbird.Count -gt 0)
    base_path       = $tbBase
    profiles_path   = Join-Path $tbBase 'Profiles'
    profile_count   = $thunderbird.Count
    default_profile = ($thunderbird | Where-Object { $_.is_default } | Select-Object -First 1).profile_name
    all_emails      = @($thunderbird | ForEach-Object { $_.emails } | Select-Object -Unique | Sort-Object)
}

# --- Outlook (HKU de cada usuario) ---
$outlook = @()
foreach ($up in $userProfiles) {
    $outlookProfiles = "$($up.registry_root)\Software\Microsoft\Office\16.0\Outlook\Profiles"
    if (-not (Test-Path $outlookProfiles)) {
        $outlookProfiles = "$($up.registry_root)\Software\Microsoft\Windows NT\CurrentVersion\Windows Messaging Subsystem\Profiles"
    }
    if (-not (Test-Path $outlookProfiles)) { continue }
    Get-ChildItem $outlookProfiles -ErrorAction SilentlyContinue | ForEach-Object {
        $profileName = $_.PSChildName
        $accountsKey = Join-Path $_.PSPath '9375C6F1-00B3-4C7E-A816-31E88A812D3A'
        $accounts = @()
        if (Test-Path $accountsKey) {
            Get-ChildItem $accountsKey -ErrorAction SilentlyContinue | ForEach-Object {
                $email = (Get-ItemProperty $_.PSPath -Name 'Email' -ErrorAction SilentlyContinue).Email
                $display = (Get-ItemProperty $_.PSPath -Name 'Display Name' -ErrorAction SilentlyContinue).'Display Name'
                if ($email) { $accounts += [ordered]@{ email = $email; display_name = $display } }
            }
        }
        $outlook += [ordered]@{ windows_user = $up.user; profile = $profileName; accounts = $accounts }
    }
}

$result = [ordered]@{
    thunderbird_summary = $tbSummary
    thunderbird         = $thunderbird
    outlook             = $outlook
}

$result | ConvertTo-Json -Compress -Depth 8
