$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

function Sanitize-Text($text) {
    if (-not $text) { return $null }
    $clean = ($text.ToString() -replace '[\x00-\x1f\x7f]', ' ' -replace '\\', '/' -replace '"', "'" -replace '\s+', ' ').Trim()
    if ($clean.Length -gt 200) { $clean.Substring(0, 200) }
    else { $clean }
}

# Coleta logins e permissoes - NUNCA extrai senhas, hashes ou tokens.

function Get-LogonTypeName($type) {
    $map = @{
        2 = 'Interativo (local)'; 3 = 'Rede'; 4 = 'Batch'; 5 = 'Serviço'
        7 = 'Unlock'; 10 = 'RDP'; 11 = 'Cache interativo'
    }
    if ($map.ContainsKey([int]$type)) { $map[[int]$type] } else { "Tipo $type" }
}

function Get-UserLocalGroups($userName) {
    $result = @()
    foreach ($grp in Get-LocalGroup -ErrorAction SilentlyContinue) {
        $members = Get-LocalGroupMember -Group $grp.Name -ErrorAction SilentlyContinue
        foreach ($m in $members) {
            $memberName = if ($m.Name -match '\\(.+)$') { $Matches[1] } else { $m.Name }
            if ($memberName -ieq $userName) { $result += $grp.Name }
        }
    }
    @($result | Select-Object -Unique)
}

# Contas locais
$localUsers = @()
try {
    $localUsers = @(Get-LocalUser -ErrorAction SilentlyContinue | ForEach-Object {
        $uname = $_.Name
        $userGroups = Get-UserLocalGroups $uname
        [ordered]@{
            name                     = $uname
            full_name                = $_.FullName
            description              = Sanitize-Text $_.Description
            enabled                  = $_.Enabled
            sid                      = $_.SID.Value
            last_logon               = if ($_.LastLogon -and $_.LastLogon.Year -gt 1970) { $_.LastLogon.ToString('o') } else { $null }
            password_last_set        = if ($_.PasswordLastSet -and $_.PasswordLastSet.Year -gt 1970) { $_.PasswordLastSet.ToString('o') } else { $null }
            password_expires         = $_.PasswordExpires
            password_never_expires   = $_.PasswordNeverExpires
            password_required        = $_.PasswordRequired
            user_may_change_password = $_.UserMayChangePassword
            account_expires          = if ($_.AccountExpires -and $_.AccountExpires.Year -lt 9000) { $_.AccountExpires.ToString('o') } else { 'Never' }
            groups                   = @($userGroups)
            is_admin                 = @($userGroups | Where-Object { $_ -match 'Administrador|Administrators' }).Count -gt 0
        }
    })
} catch {
    $localUsers = @(Get-CimInstance Win32_UserAccount -Filter "LocalAccount=True" | ForEach-Object {
        [ordered]@{
            name = $_.Name; enabled = -not $_.Disabled; sid = $_.SID
            description = $_.FullName; groups = @(); is_admin = $null
        }
    })
}

# Grupos e membros
$privilegeGroups = @(
    'Administrators', 'Administradores', 'Users', 'Usuários', 'Guests', 'Convidados',
    'Remote Desktop Users', 'Usuários da área de trabalho remota',
    'Power Users', 'Backup Operators', 'Hyper-V Administrators', 'Administradores do Hyper-V'
)

$groups = @()
foreach ($g in Get-LocalGroup -ErrorAction SilentlyContinue) {
    $members = @(Get-LocalGroupMember -Group $g.Name -ErrorAction SilentlyContinue | ForEach-Object {
        [ordered]@{
            name             = $_.Name
            object_class     = $_.ObjectClass
            principal_source = $_.PrincipalSource.ToString()
            sid              = $_.SID.Value
        }
    })
    $groups += [ordered]@{
        name                = $g.Name
        sid                 = $g.SID.Value
        description         = Sanitize-Text $g.Description
        members             = $members
        is_privileged_group = ($privilegeGroups -contains $g.Name)
    }
}

# Sessões ativas
$activeSessions = @()
try {
    $activeSessions = @(query user 2>$null | Select-Object -Skip 1 | ForEach-Object {
        $line = $_ -replace '^\s*>', ' ' -replace '\s{2,}', '|'
        $parts = $line -split '\|' | Where-Object { $_ -ne '' }
        if ($parts.Count -ge 4) {
            [ordered]@{
                user        = $parts[0].Trim()
                session     = $parts[1].Trim()
                id          = $parts[2].Trim()
                state       = $parts[3].Trim()
                idle_time   = if ($parts.Count -gt 4) { $parts[4].Trim() } else { $null }
                logon_time  = if ($parts.Count -gt 5) { ($parts[5..($parts.Count-1)] -join ' ').Trim() } else { $null }
            }
        }
    } | Where-Object { $_ })
} catch {}

if ($activeSessions.Count -eq 0) {
    try {
        $loggedOn = Get-CimInstance Win32_LoggedOnUser -ErrorAction SilentlyContinue
        foreach ($entry in $loggedOn) {
            $session = Get-CimInstance Win32_LogonSession -Filter "LogonId='$($entry.Dependent.LogonId)'" -ErrorAction SilentlyContinue
            $activeSessions += [ordered]@{
                user       = ($entry.Antecedent.Name -replace '.*\\', '')
                domain     = ($entry.Antecedent.Name -replace '\\.*', '')
                logon_type = if ($session) { Get-LogonTypeName $session.LogonType } else { $null }
                start_time = if ($session) { $session.StartTime.ToString('o') } else { $null }
            }
        }
    } catch {}
}

# Histórico recente de logons (Event ID 4624) — requer permissão no log Security
$recentLogons = @()
try {
    $events = Get-WinEvent -FilterHashtable @{
        LogName = 'Security'; Id = 4624
    } -MaxEvents 15 -ErrorAction SilentlyContinue
    foreach ($ev in $events) {
        $xml = [xml]$ev.ToXml()
        $d = @{}
        foreach ($n in $xml.Event.EventData.Data) { $d[$n.Name] = $n.'#text' }
        $recentLogons += [ordered]@{
            time       = $ev.TimeCreated.ToString('o')
            user       = $d.TargetUserName
            domain     = $d.TargetDomainName
            logon_type = Get-LogonTypeName $d.LogonType
            source_ip  = $d.IpAddress
            workstation = $d.WorkstationName
        }
    }
} catch {
    $recentLogons = @([ordered]@{ note = 'Historico indisponivel (requer execucao como Administrador)' })
}

# Credenciais salvas no Windows (Gerenciador de Credenciais) - somente alvo e usuario
$savedCredentials = @()
try {
    $cmdkeyOut = cmdkey /list 2>$null
    $curTarget = $null; $curType = $null; $curUser = $null
    foreach ($line in $cmdkeyOut) {
        if ($line -match 'Target:\s*(.+)') {
            if ($curTarget) {
                $savedCredentials += [ordered]@{ Target = $curTarget; Type = $curType; User = $curUser }
            }
            $curTarget = $Matches[1].Trim(); $curType = $null; $curUser = $null
        }
        elseif ($line -match 'Type:\s*(.+)') { $curType = $Matches[1].Trim() }
        elseif ($line -match 'User:\s*(.+)') { $curUser = $Matches[1].Trim() }
    }
    if ($curTarget) {
        $savedCredentials += [ordered]@{ Target = $curTarget; Type = $curType; User = $curUser }
    }
} catch {}

# Auto-logon (somente usuário — NUNCA a senha)
$autoLogon = [ordered]@{ enabled = $false }
try {
    $winlogon = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon' -ErrorAction SilentlyContinue
    if ($winlogon.AutoAdminLogon -eq 1) {
        $autoLogon = [ordered]@{
            enabled  = $true
            username = $winlogon.DefaultUserName
            domain   = $winlogon.DefaultDomainName
        }
    }
} catch {}

# Política de senha local
$passwordPolicy = $null
try {
    $netAccounts = net accounts 2>$null
    if ($netAccounts) {
        $passwordPolicy = [ordered]@{ lines = @($netAccounts | ForEach-Object { Sanitize-Text $_ }) }
        foreach ($line in $netAccounts) {
            if ($line -match 'comprimento minimo|comprimento mínimo|minimum password length') {
                if ($line -match '(\d+)') { $passwordPolicy.min_length = [int]$Matches[1] }
            }
        }
    }
} catch {}

# Domínio / workgroup
$computerSystem = Get-CimInstance Win32_ComputerSystem
$domainInfo = [ordered]@{
    domain         = $computerSystem.Domain
    workgroup      = $computerSystem.Workgroup
    part_of_domain = $computerSystem.PartOfDomain
    logon_server   = $env:LOGONSERVER
}

# Direitos de logon (quem pode RDP, etc.)
$logonRights = @()
try {
    $rdpUsers = @(Get-LocalGroupMember -Group 'Remote Desktop Users' -ErrorAction SilentlyContinue | ForEach-Object {
        [ordered]@{ user = $_.Name; source = $_.PrincipalSource.ToString() }
    })
    $logonRights += [ordered]@{ right = 'Remote Desktop (RDP)'; allowed_users = $rdpUsers }
    $admins = @(Get-LocalGroupMember -Group 'Administrators' -ErrorAction SilentlyContinue | ForEach-Object {
        [ordered]@{ user = $_.Name; source = $_.PrincipalSource.ToString() }
    })
    $logonRights += [ordered]@{ right = 'Administrador local'; allowed_users = $admins }
} catch {}

$result = [ordered]@{
    domain_info        = $domainInfo
    local_users        = $localUsers
    local_groups       = $groups
    active_sessions    = $activeSessions
    recent_logons      = $recentLogons
    saved_credentials  = $savedCredentials
    auto_logon         = $autoLogon
    password_policy    = $passwordPolicy
    logon_rights       = $logonRights
    collection_note    = 'Senhas e hashes NAO sao coletados - apenas logins, grupos e metadados.'
}

$result | ConvertTo-Json -Compress -Depth 8
