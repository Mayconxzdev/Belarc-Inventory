$ErrorActionPreference = 'SilentlyContinue'

. (Join-Path $PSScriptRoot '_heavy-apps.ps1')
$heavyCfg = Get-HeavyAppsConfig -BaseDir $PSScriptRoot
$knownApps = @($heavyCfg.apps)

function Get-EventFields($ev) {
    $xml = [xml]$ev.ToXml()
    $data = @{}
    foreach ($n in $xml.Event.EventData.Data) {
        if ($n.Name) { $data[$n.Name] = $n.'#text' }
    }
    $msg = if ($ev.Message) { ($ev.Message -replace '\s+', ' ').Trim() } else { '' }
    if ($msg.Length -gt 500) { $msg = $msg.Substring(0, 500) }
    [ordered]@{
        time         = $ev.TimeCreated.ToString('o')
        log          = $ev.LogName
        level        = $ev.LevelDisplayName
        id           = $ev.Id
        provider     = $ev.ProviderName
        message      = $msg
        machine      = $ev.MachineName
        data         = $data
    }
}

function Parse-BugCheck($message, $data) {
    $code = $null; $param1 = $null; $param2 = $null; $param3 = $null; $param4 = $null
    if ($message -match 'bugcheck code[:\s]+(0x[0-9a-fA-F]+)') { $code = $Matches[1] }
    elseif ($message -match 'BugcheckCode[:\s]+(0x[0-9a-fA-F]+)') { $code = $Matches[1] }
    if ($data.BugcheckCode) { $code = '0x{0:X8}' -f [int]$data.BugcheckCode }
    if ($data.BugcheckParameter1) { $param1 = $data.BugcheckParameter1 }
    if ($data.BugcheckParameter2) { $param2 = $data.BugcheckParameter2 }
    if ($data.BugcheckParameter3) { $param3 = $data.BugcheckParameter3 }
    if ($data.BugcheckParameter4) { $param4 = $data.BugcheckParameter4 }
    [ordered]@{ code = $code; param1 = $param1; param2 = $param2; param3 = $param3; param4 = $param4 }
}

function Get-AppNameFromEvent($entry) {
    $candidates = @(
        $entry.data.AppName,
        $entry.data.Application,
        $entry.data.P1,
        $entry.data.P2,
        $entry.message
    ) | Where-Object { $_ -and $_.ToString().Trim().Length -gt 1 }
    foreach ($c in $candidates) {
        $s = $c.ToString().Trim()
        if ($s -match '([\w\-\.]+\.(exe|bin))') { return $Matches[1] }
        if ($s.Length -le 120) { return $s }
    }
    if ($entry.provider) { return $entry.provider }
    'desconhecido'
}

function Classify-AppEvent($eid) {
    switch ($eid) {
        1000 { 'app_crash' }
        1002 { 'app_hang' }
        1001 { 'app_error_report' }
        1006 { 'app_wer' }
        default { 'app_event' }
    }
}

function Add-TimelineEvent($timeline, $entry) {
    $timeline += $entry
    $timeline
}

$daysBack = 14
$todayStart = (Get-Date).Date
$startTime = (Get-Date).AddDays(-$daysBack)

# --- Tela azul / falhas criticas de sistema ---
$bsodEvents = @()
$bsodIds = @(1001, 41, 6008, 1074, 1076)

foreach ($eid in $bsodIds) {
    try {
        $evs = Get-WinEvent -FilterHashtable @{
            LogName   = 'System'
            Id        = $eid
            StartTime = $startTime
        } -MaxEvents 20 -ErrorAction SilentlyContinue
        foreach ($ev in $evs) {
            $entry = Get-EventFields $ev
            $category = switch ($eid) {
                1001 { 'BSOD_BUGCHECK' }
                41   { 'KERNEL_POWER_UNEXPECTED' }
                6008 { 'UNEXPECTED_SHUTDOWN' }
                1074 { 'SHUTDOWN_INITIATED' }
                1076 { 'RELIABILITY_FAULT' }
                default { 'SYSTEM_CRITICAL' }
            }
            $entry.category = $category
            if ($eid -eq 1001) {
                $entry.bugcheck = Parse-BugCheck $ev.Message $entry.data
                $entry.summary = if ($entry.bugcheck.code) { "Tela azul: $($entry.bugcheck.code)" } else { 'Tela azul (BugCheck)' }
            }
            elseif ($eid -eq 41) {
                $entry.summary = 'Desligamento inesperado / possivel tela azul ou queda de energia'
            }
            elseif ($eid -eq 6008) {
                $entry.summary = 'Desligamento anterior inesperado'
            }
            else { $entry.summary = ($ev.Message -replace '\s+', ' ').Substring(0, [Math]::Min(120, $ev.Message.Length)) }
            $bsodEvents += $entry
        }
    } catch {}
}

$bsodEvents = @($bsodEvents | Sort-Object { $_.time } -Descending | Select-Object -First 30)

# --- Erros System (ultimos 14 dias) ---
$systemErrors = @()
try {
    $systemErrors = @(Get-WinEvent -FilterHashtable @{
        LogName   = 'System'
        Level     = 2
        StartTime = $startTime
    } -MaxEvents 50 -ErrorAction SilentlyContinue | ForEach-Object {
        $e = Get-EventFields $_
        $e.category = 'SYSTEM_ERROR'
        $e
    })
} catch {}

# --- Erros de disco / armazenamento (SSD/HDD) ---
$diskEvents = @()
$diskEventIds = @(7, 11, 51, 52, 153, 154, 157, 129, 130, 140, 141)
foreach ($eid in $diskEventIds) {
    try {
        $evs = Get-WinEvent -FilterHashtable @{
            LogName   = 'System'
            Id        = $eid
            StartTime = $startTime
        } -MaxEvents 15 -ErrorAction SilentlyContinue
        foreach ($ev in $evs) {
            $e = Get-EventFields $ev
            $e.category = 'DISK_ERROR'
            $e.summary = "Erro de disco/armazenamento (ID $eid): $($e.message.Substring(0, [Math]::Min(100, $e.message.Length)))"
            $diskEvents += $e
        }
    } catch {}
}
$diskEvents = @($diskEvents | Sort-Object { $_.time } -Descending | Select-Object -First 40)

# --- WHEA (hardware / temperatura) ---
$wheaEvents = @()
try {
    $wheaEvents = @(Get-WinEvent -FilterHashtable @{
        LogName   = 'System'
        ProviderName = 'Microsoft-Windows-WHEA-Logger'
        StartTime = $startTime
    } -MaxEvents 20 -ErrorAction SilentlyContinue | ForEach-Object {
        $e = Get-EventFields $_
        $e.category = 'WHEA_HARDWARE'
        $e.summary = "Hardware/WHEA: $($e.message.Substring(0, [Math]::Min(120, $e.message.Length)))"
        $e
    })
} catch {}

# --- Erros Application ---
$appErrors = @()
try {
    $appErrors = @(Get-WinEvent -FilterHashtable @{
        LogName   = 'Application'
        Level     = 2
        StartTime = $startTime
    } -MaxEvents 40 -ErrorAction SilentlyContinue | ForEach-Object {
        $e = Get-EventFields $_
        $e.category = 'APPLICATION_ERROR'
        $e
    })
} catch {}

# --- Travamentos e falhas de aplicativos (Event IDs 1000, 1002, 1001) ---
$appCrashHangEvents = @()
foreach ($eid in @(1000, 1002, 1001, 1006)) {
    try {
        $evs = Get-WinEvent -FilterHashtable @{
            LogName   = 'Application'
            Id        = $eid
            StartTime = $startTime
        } -MaxEvents 40 -ErrorAction SilentlyContinue
        foreach ($ev in $evs) {
            $e = Get-EventFields $ev
            $e.category = Classify-AppEvent $eid
            $e.app_name = Get-AppNameFromEvent $e
            $e.fault_module = $e.data.FaultModuleName
            $e.exception_code = $e.data.ExceptionCode
            if ($eid -eq 1002) {
                $e.summary = "Travamento (hang): $($e.app_name)"
            } elseif ($eid -eq 1000) {
                $code = if ($e.exception_code) { " cod $($e.exception_code)" } else { '' }
                $e.summary = "Falha: $($e.app_name)$code"
            } else {
                $e.summary = "Evento app ($eid): $($e.app_name)"
            }
            $appCrashHangEvents += $e
        }
    } catch {}
}
$appCrashHangEvents = @($appCrashHangEvents | Sort-Object { $_.time } -Descending | Select-Object -First 80)

# --- Eventos de apps criticos (OperationsSuite, Fusion, AutoCAD, LibreOffice) ---
$criticalAppEvents = @()
foreach ($ev in $appCrashHangEvents) {
    $matchText = "$($ev.app_name) $($ev.message) $($ev.provider) $($ev.data.AppName)"
    $matched = Find-HeavyApp -Text $matchText -Apps $knownApps
    if ($matched) {
        $criticalAppEvents += [ordered]@{
            time            = $ev.time
            event_id        = $ev.id
            category        = if ($ev.id -eq 1002) { 'known_app_hang' } else { 'known_app_crash' }
            severity        = if ($ev.id -eq 1002) { 'warning' } else { 'warning' }
            app_id          = $matched.id
            app_label       = $matched.label
            app_name        = $ev.app_name
            fault_module    = $ev.fault_module
            exception_code  = $ev.exception_code
            message         = $ev.summary
            detail          = $ev.message
            log             = $ev.log
            provider        = $ev.provider
            remediation     = $matched.remediation
            event_key       = "evt|$($matched.id)|$($ev.time)|$($ev.id)|$($ev.app_name)"
        }
    }
}

# --- Falhas de servico ---
$serviceFailures = @()
foreach ($eid in @(7031, 7034, 7023)) {
    try {
        $evs = Get-WinEvent -FilterHashtable @{
            LogName = 'System'; Id = $eid; StartTime = $startTime
        } -MaxEvents 10 -ErrorAction SilentlyContinue
        foreach ($ev in $evs) {
            $e = Get-EventFields $ev
            $e.category = 'SERVICE_FAILURE'
            $serviceFailures += $e
        }
    } catch {}
}

# --- Minidumps (arquivos de tela azul) ---
$minidumps = @()
$dumpPath = "$env:SystemRoot\Minidump"
if (Test-Path $dumpPath) {
    $minidumps = @(Get-ChildItem $dumpPath -Filter '*.dmp' -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending | Select-Object -First 10 | ForEach-Object {
        [ordered]@{
            file          = $_.Name
            size_kb       = [math]::Round($_.Length / 1KB, 1)
            created       = $_.CreationTime.ToString('o')
            last_modified = $_.LastWriteTime.ToString('o')
        }
    })
}

# --- Linha do tempo do dia (hoje) ---
$timelineToday = @()

function Add-TimelineRow($list, $timeStr, $category, $severity, $title, $detail, $extra) {
    $script:today = (Get-Date).Date
    try { $dt = [datetimeoffset]::Parse($timeStr).DateTime } catch { return $list }
    if ($dt -lt $script:today) { return $list }
    $row = [ordered]@{
        time     = $timeStr
        hour     = $dt.Hour
        category = $category
        severity = $severity
        title    = $title
        detail   = $detail
    }
    foreach ($k in $extra.Keys) { $row[$k] = $extra[$k] }
    $list += $row
    $list
}

foreach ($ev in $criticalAppEvents) {
    if ([datetimeoffset]::Parse($ev.time).DateTime -ge $todayStart) {
        $timelineToday = Add-TimelineRow $timelineToday $ev.time $ev.category $ev.severity $ev.message $ev.detail ([ordered]@{
            event_id = $ev.event_id; app_label = $ev.app_label; app_id = $ev.app_id
            exception_code = $ev.exception_code; fault_module = $ev.fault_module
        })
    }
}
foreach ($ev in $diskEvents) {
    if ([datetimeoffset]::Parse($ev.time).DateTime -ge $todayStart) {
        $timelineToday = Add-TimelineRow $timelineToday $ev.time 'disk_error' 'critical' $ev.summary $ev.message ([ordered]@{
            event_id = $ev.id; log = $ev.log
        })
    }
}
foreach ($ev in $wheaEvents) {
    if ([datetimeoffset]::Parse($ev.time).DateTime -ge $todayStart) {
        $timelineToday = Add-TimelineRow $timelineToday $ev.time 'whea_hardware' 'warning' $ev.summary $ev.message ([ordered]@{
            event_id = $ev.id
        })
    }
}
foreach ($ev in ($bsodEvents | Where-Object { $_.category -in @('BSOD_BUGCHECK', 'KERNEL_POWER_UNEXPECTED', 'UNEXPECTED_SHUTDOWN') })) {
    if ([datetimeoffset]::Parse($ev.time).DateTime -ge $todayStart) {
        $cat = if ($ev.category -eq 'BSOD_BUGCHECK') { 'bsod' } else { 'unexpected_shutdown' }
        $timelineToday = Add-TimelineRow $timelineToday $ev.time $cat 'critical' $ev.summary $ev.message ([ordered]@{
            event_id = $ev.id; bugcheck = $ev.bugcheck.code
        })
    }
}

$timelineToday = @($timelineToday | Sort-Object { $_.time } -Descending)

$timelineByHour = @{}
foreach ($row in $timelineToday) {
    $h = [string]$row.hour
    if (-not $timelineByHour.ContainsKey($h)) { $timelineByHour[$h] = @() }
    $timelineByHour[$h] += $row
}
$hourlyBuckets = @($timelineByHour.GetEnumerator() | Sort-Object { [int]$_.Name } -Descending | ForEach-Object {
    [ordered]@{ hour = [int]$_.Name; events = @($_.Value | Sort-Object { $_.time } -Descending) }
})

$lastBsod = $bsodEvents | Where-Object { $_.category -in @('BSOD_BUGCHECK', 'KERNEL_POWER_UNEXPECTED', 'UNEXPECTED_SHUTDOWN') } | Select-Object -First 1

$summary = [ordered]@{
    period_days                = $daysBack
    bsod_count                 = @($bsodEvents | Where-Object { $_.category -eq 'BSOD_BUGCHECK' }).Count
    unexpected_shutdowns       = @($bsodEvents | Where-Object { $_.category -in @('KERNEL_POWER_UNEXPECTED', 'UNEXPECTED_SHUTDOWN') }).Count
    system_errors_count        = $systemErrors.Count
    application_errors_count   = $appErrors.Count
    app_crash_hang_count       = $appCrashHangEvents.Count
    critical_app_events_count  = $criticalAppEvents.Count
    critical_app_events_today  = @($criticalAppEvents | Where-Object { [datetimeoffset]::Parse($_.time).DateTime -ge $todayStart }).Count
    disk_errors_count          = $diskEvents.Count
    whea_events_count          = $wheaEvents.Count
    service_failures_count     = $serviceFailures.Count
    minidump_count             = $minidumps.Count
    timeline_today_count       = $timelineToday.Count
    has_recent_bsod            = ($null -ne $lastBsod)
    last_bsod_time             = if ($lastBsod) { $lastBsod.time } else { $null }
    last_bsod_summary          = if ($lastBsod) { $lastBsod.summary } else { $null }
    last_bugcheck_code         = if ($lastBsod -and $lastBsod.bugcheck) { $lastBsod.bugcheck.code } else { $null }
}

$result = [ordered]@{
    summary              = $summary
    bsod_events          = $bsodEvents
    system_errors        = $systemErrors
    disk_events          = $diskEvents
    whea_events          = $wheaEvents
    application_errors   = $appErrors
    app_crash_hang       = $appCrashHangEvents
    critical_app_events  = $criticalAppEvents
    service_failures     = $serviceFailures
    minidumps            = $minidumps
    daily_timeline       = [ordered]@{
        date          = (Get-Date).ToString('yyyy-MM-dd')
        event_count   = $timelineToday.Count
        events        = $timelineToday
        by_hour       = $hourlyBuckets
    }
}

$result | ConvertTo-Json -Compress -Depth 10
