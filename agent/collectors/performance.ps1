$ErrorActionPreference = 'SilentlyContinue'

function Get-SampleCounter($path, $samples = 2, $intervalSec = 1) {
    try {
        $c = Get-Counter -Counter $path -SampleInterval $intervalSec -MaxSamples $samples -ErrorAction Stop
        $vals = @($c.CounterSamples | ForEach-Object { [double]$_.CookedValue })
        if ($vals.Count -eq 0) { return $null }
        [math]::Round(($vals | Measure-Object -Average).Average, 1)
    } catch { $null }
}

function Get-CounterSamples($path, $samples = 5, $intervalSec = 1) {
    $out = @()
    try {
        $c = Get-Counter -Counter $path -SampleInterval $intervalSec -MaxSamples $samples -ErrorAction Stop
        $base = Get-Date
        $i = 0
        foreach ($s in $c.CounterSamples) {
            $out += [ordered]@{
                time  = $base.AddSeconds(-($samples - $i - 1) * $intervalSec).ToString('o')
                value = [math]::Round([double]$s.CookedValue, 1)
            }
            $i++
        }
    } catch {}
    $out
}

function Get-CpuLoadPercent {
    $wmi = @(Get-CimInstance Win32_Processor | ForEach-Object { $_.LoadPercentage } | Where-Object { $_ -ne $null })
    if ($wmi.Count -gt 0) {
        return [math]::Round(($wmi | Measure-Object -Average).Average, 1)
    }
    Get-SampleCounter '\Processor(_Total)\% Processor Time'
}

$collectedAt = (Get-Date).ToString('o')
$todayStart = (Get-Date).Date
$daysBack = 14
$startTime = (Get-Date).AddDays(-$daysBack)

. (Join-Path $PSScriptRoot '_heavy-apps.ps1')
$heavyCfg = Get-HeavyAppsConfig -BaseDir $PSScriptRoot
$companyProfile = $heavyCfg.profile
$erpServerIp = $heavyCfg.erp_ip
$erpName = $heavyCfg.erp_name
$knownHeavyApps = @($heavyCfg.apps)

# --- CPU ---
$cpuLoad = Get-CpuLoadPercent
$cpuCores = (Get-CimInstance Win32_Processor | Measure-Object -Property NumberOfLogicalProcessors -Sum).Sum

# --- Memória ---
$os = Get-CimInstance Win32_OperatingSystem
$memTotalGb = if ($os.TotalVisibleMemorySize) { [math]::Round($os.TotalVisibleMemorySize / 1MB, 2) } else { $null }
$memFreeGb = if ($os.FreePhysicalMemory) { [math]::Round($os.FreePhysicalMemory / 1MB, 2) } else { $null }
$memUsedPct = if ($memTotalGb -gt 0 -and $null -ne $memFreeGb) {
    [math]::Round((($memTotalGb - $memFreeGb) / $memTotalGb) * 100, 1)
} else { $null }

# --- Discos lógicos ---
$logicalDisks = @(Get-CimInstance Win32_LogicalDisk | Where-Object { $_.DriveType -eq 3 } | ForEach-Object {
    $freePct = if ($_.Size -gt 0) { [math]::Round(($_.FreeSpace / $_.Size) * 100, 1) } else { 0 }
    [ordered]@{
        letter       = $_.DeviceID
        size_gb      = [math]::Round($_.Size / 1GB, 2)
        free_gb      = [math]::Round($_.FreeSpace / 1GB, 2)
        free_percent = $freePct
    }
})

# --- Disco físico: amostras ao longo de ~5s (SSD/HDD 100%) ---
$diskTimeSamples = Get-CounterSamples '\PhysicalDisk(_Total)\% Disk Time' 5 1
$diskQueueSamples = Get-CounterSamples '\PhysicalDisk(_Total)\Avg. Disk Queue Length' 5 1
$diskTimeTotal = if ($diskTimeSamples.Count) {
    [math]::Round(($diskTimeSamples | ForEach-Object { $_.value } | Measure-Object -Maximum).Maximum, 1)
} else { Get-SampleCounter '\PhysicalDisk(_Total)\% Disk Time' }
$diskQueue = if ($diskQueueSamples.Count) {
    [math]::Round(($diskQueueSamples | ForEach-Object { $_.value } | Measure-Object -Maximum).Maximum, 1)
} else { Get-SampleCounter '\PhysicalDisk(_Total)\Avg. Disk Queue Length' }

$physicalDisks = @()
try {
    $pdCounters = Get-Counter '\PhysicalDisk(*)\% Disk Time' -ErrorAction Stop
    foreach ($s in $pdCounters.CounterSamples) {
        if ($s.InstanceName -match '^_Total|HarddiskVolume') { continue }
        $physicalDisks += [ordered]@{
            instance          = $s.InstanceName
            disk_time_percent = [math]::Round([double]$s.CookedValue, 1)
        }
    }
} catch {}

# --- Temperaturas (instantaneo + historico do dia via coleta) ---
$temperatures = @()
try {
    Get-CimInstance -Namespace root/wmi -ClassName MSAcpi_ThermalZoneTemperature -ErrorAction Stop |
        ForEach-Object {
            $c = if ($_.CurrentTemperature) { [math]::Round(($_.CurrentTemperature / 10) - 273.15, 1) } else { $null }
            if ($null -ne $c) {
                $temperatures += [ordered]@{ source = 'ThermalZone'; celsius = $c; instance = $_.InstanceName; observed_at = $collectedAt }
            }
        }
} catch {}
try {
    Get-PhysicalDisk -ErrorAction Stop | ForEach-Object {
        $rel = Get-StorageReliabilityCounter -PhysicalDisk $_ -ErrorAction SilentlyContinue
        if ($rel -and $rel.Temperature) {
            $temperatures += [ordered]@{
                source       = 'StorageReliability'
                celsius      = [math]::Round($rel.Temperature, 1)
                instance     = $_.FriendlyName
                observed_at  = $collectedAt
                read_errors  = $rel.ReadErrorsTotal
                write_errors = $rel.WriteErrorsTotal
                wear_percent = if ($null -ne $rel.Wear) { $rel.Wear } else { $null }
            }
        }
    }
} catch {}

# --- Top processos (CPU / RAM) ---
$topCpu = @()
$topMem = @()
try {
    $procs = Get-Process | Where-Object { $_.Id -gt 4 } | Sort-Object CPU -Descending | Select-Object -First 8
    foreach ($p in $procs) {
        $topCpu += [ordered]@{
            name        = $p.ProcessName
            pid         = $p.Id
            cpu_seconds = [math]::Round($p.CPU, 1)
            memory_mb   = [math]::Round($p.WorkingSet64 / 1MB, 1)
        }
    }
    $procsMem = Get-Process | Where-Object { $_.Id -gt 4 } | Sort-Object WorkingSet64 -Descending | Select-Object -First 8
    foreach ($p in $procsMem) {
        $topMem += [ordered]@{
            name      = $p.ProcessName
            pid       = $p.Id
            memory_mb = [math]::Round($p.WorkingSet64 / 1MB, 1)
        }
    }
} catch {}

# --- Reliability Monitor (travamentos, falhas de app, hangs) ---
$reliabilityRecords = @()
$recordTypeMap = @{
    0 = 'SoftwareChange'
    1 = 'ApplicationFailure'
    2 = 'WindowsFailure'
    3 = 'DegradedStartup'
    4 = 'BugCheck'
    5 = 'Shutdown'
    6 = 'Boot'
    7 = 'Checkpoint'
    8 = 'Update'
    9 = 'Other'
}
try {
    Get-CimInstance Win32_ReliabilityRecords -ErrorAction Stop |
        Where-Object { $_.TimeGenerated -ge $startTime } |
        Sort-Object TimeGenerated -Descending |
        Select-Object -First 60 |
        ForEach-Object {
            $rtype = [int]$_.RecordType
            $src = $_.SourceName
            $mapped = $recordTypeMap[$rtype]
            if ($rtype -eq 0) {
                if ($src -match 'Application Error') { $mapped = 'ApplicationFailure'; $rtype = 1 }
                elseif ($src -match 'Application Hang') { $mapped = 'ApplicationHang'; $rtype = 10 }
                elseif ($src -match 'Windows Error') { $mapped = 'WindowsFailure'; $rtype = 2 }
            }
            $reliabilityRecords += [ordered]@{
                time           = $_.TimeGenerated.ToString('o')
                record_type    = $mapped
                record_type_id = $rtype
                source         = $src
                product        = $_.ProductName
                message        = if ($_.Message) { ($_.Message -replace '\s+', ' ').Substring(0, [Math]::Min(200, $_.Message.Length)) } else { '' }
                event_id       = $_.EventIdentifier
            }
        }
} catch {}

# --- Sinais detectados na coleta (thresholds) ---
$signals = @()
$recs = @{
    cpu_high        = 'Verificar processos com alto uso de CPU; considerar reinício ou desinstalar software pesado.'
    memory_high     = 'Fechar programas não usados; avaliar upgrade de RAM se recorrente.'
    disk_critical   = 'Liberar espaco imediatamente - disco quase cheio causa lentidao e falhas.'
    disk_low        = 'Planejar limpeza de disco; menos de 15% livre afeta performance.'
    disk_saturation = 'Disco em 100% de uso - SSD/HDD saturado; verificar antivirus, backup ou indexacao.'
    disk_queue      = 'Fila de disco alta - possivel lag; verificar processos de I/O.'
    temp_high       = 'Temperatura elevada - limpar ventilacao, verificar cooler e pasta termica.'
    temp_critical   = 'Temperatura critica - risco de throttling ou desligamento; acao imediata.'
    app_crash       = 'Aplicativo travou ou falhou - reinstalar ou atualizar o programa.'
    system_failure  = 'Falha do Windows registrada - correlacionar com Event Viewer e BSOD.'
}

if ($cpuLoad -ge 90) {
    $signals += [ordered]@{
        type = 'cpu_high'; severity = 'warning'; value = $cpuLoad; threshold = 90
        message = "CPU em ${cpuLoad}% - possivel lag ou travamento"
        recommendation = $recs.cpu_high; observed_at = $collectedAt
    }
} elseif ($cpuLoad -ge 75) {
    $signals += [ordered]@{
        type = 'cpu_elevated'; severity = 'info'; value = $cpuLoad; threshold = 75
        message = "CPU elevada: $cpuLoad%"; recommendation = 'Monitorar se ocorre com frequência.'
        observed_at = $collectedAt
    }
}

if ($memUsedPct -ge 90) {
    $signals += [ordered]@{
        type = 'memory_high'; severity = 'warning'; value = $memUsedPct; threshold = 90
        message = "Memoria em ${memUsedPct}% - risco de lentidao"
        recommendation = $recs.memory_high; observed_at = $collectedAt
    }
}

foreach ($ld in $logicalDisks) {
    if ($ld.free_percent -lt 5) {
        $signals += [ordered]@{
            type = 'disk_critical'; severity = 'critical'; value = $ld.free_percent; threshold = 5
            message = "Disco $($ld.letter) com $($ld.free_percent)% livre - critico"
            recommendation = $recs.disk_critical; observed_at = $collectedAt
        }
    } elseif ($ld.free_percent -lt 15) {
        $signals += [ordered]@{
            type = 'disk_low'; severity = 'warning'; value = $ld.free_percent; threshold = 15
            message = "Disco $($ld.letter) com $($ld.free_percent)% livre"
            recommendation = $recs.disk_low; observed_at = $collectedAt
        }
    }
}

foreach ($sample in $diskTimeSamples) {
    if ($sample.value -ge 85) {
        $signals += [ordered]@{
            type = 'disk_saturation'; severity = 'warning'; value = $sample.value; threshold = 85
            message = "Disco fisico em $($sample.value)% Disk Time (amostra)"
            recommendation = $recs.disk_saturation
            observed_at = $sample.time
            event_key = "disk|sat|$($sample.time)|$($sample.value)"
        }
    }
}
if ($diskTimeTotal -ge 90 -and -not ($signals | Where-Object { $_.type -eq 'disk_saturation' })) {
    $signals += [ordered]@{
        type = 'disk_saturation'; severity = 'warning'; value = $diskTimeTotal; threshold = 90
        message = "Disco fisico em $diskTimeTotal% de uso (possivel SSD 100%)"
        recommendation = $recs.disk_saturation; observed_at = $collectedAt
        event_key = "disk|sat|total|$diskTimeTotal"
    }
}
foreach ($pd in $physicalDisks) {
    if ($pd.disk_time_percent -ge 85) {
        $signals += [ordered]@{
            type = 'disk_saturation'; severity = 'warning'; value = $pd.disk_time_percent; threshold = 85
            message = "Disco $($pd.instance) em $($pd.disk_time_percent)% Disk Time"
            recommendation = $recs.disk_saturation; observed_at = $collectedAt
            event_key = "disk|sat|$($pd.instance)|$($pd.disk_time_percent)"
        }
    }
}
if ($diskQueue -ge 2) {
    $signals += [ordered]@{
        type = 'disk_queue'; severity = 'warning'; value = $diskQueue; threshold = 2
        message = "Fila de disco: ${diskQueue} - possivel delay de I/O"
        recommendation = $recs.disk_queue; observed_at = $collectedAt
    }
}

foreach ($t in $temperatures) {
    if ($t.celsius -ge 90) {
        $signals += [ordered]@{
            type = 'temp_critical'; severity = 'critical'; value = $t.celsius; threshold = 90
            message = "Temperatura $($t.celsius)C ($($t.source) / $($t.instance))"
            recommendation = $recs.temp_critical; observed_at = $collectedAt
            event_key = "temp|$($t.instance)|$($t.celsius)"
        }
    } elseif ($t.celsius -ge 80) {
        $signals += [ordered]@{
            type = 'temp_high'; severity = 'warning'; value = $t.celsius; threshold = 80
            message = "Temperatura $($t.celsius)C ($($t.source) / $($t.instance))"
            recommendation = $recs.temp_high; observed_at = $collectedAt
            event_key = "temp|$($t.instance)|$($t.celsius)"
        }
    }
}

# --- Servidor ERP (LAN) ---
$erpReachable = $null
$erpLatencyMs = $null
try {
    $erpPing = Test-Connection -ComputerName $erpServerIp -Count 2 -ErrorAction Stop
    $erpReachable = $true
    $erpLatencyMs = ($erpPing | Measure-Object -Property ResponseTime -Average).Average
} catch {
    $erpReachable = $false
}
$firstErpBranch = @($companyProfile.erp_branches)[0]
if ($erpReachable -eq $false) {
    $signals += [ordered]@{
        type = 'erp_unreachable'; severity = 'warning'; value = $null; threshold = $null
        app_id = $(if ($firstErpBranch) { $firstErpBranch.id } else { 'erp_branch_a' })
        message = "Servidor ERP $erpServerIp inalcancavel - $erpName pode travar ou ficar lento"
        recommendation = "Verificar cabo de rede, switch e servidor ERP ligado. ping $erpServerIp no CMD."
        event_key = "erp|$erpServerIp|$(Get-Date -Format 'yyyy-MM-dd')"
        observed_at = $collectedAt
    }
}

# --- Instancias dos programas criticos em execucao ---
$knownAppStatus = @()
foreach ($ka in $knownHeavyApps) {
    $running = @()
    foreach ($pn in @($ka.procs)) {
        Get-Process -Name $pn -ErrorAction SilentlyContinue | ForEach-Object {
            $running += [ordered]@{
                name = $_.ProcessName; pid = $_.Id
                memory_mb = [math]::Round($_.WorkingSet64 / 1MB, 1)
                cpu_seconds = [math]::Round($_.CPU, 1)
            }
        }
    }
    $installed = if ($ka.path) { Test-Path $ka.path } else { $running.Count -gt 0 }
    $knownAppStatus += [ordered]@{
        id = $ka.id; label = $ka.label; installed = $installed
        running_count = $running.Count; processes = $running; remediation = $ka.remediation
    }
}

# Travamentos de programas criticos (Reliability Monitor)
foreach ($rr in $reliabilityRecords | Where-Object { $_.record_type_id -in 1, 2, 4, 10 }) {
    $prod = ($_.product + ' ' + $_.source + ' ' + $_.message).ToLower()
    $matchedApp = Find-HeavyApp -Text $prod -Apps $knownHeavyApps
    if ($matchedApp) {
        $sigType = if ($_.record_type_id -eq 10) { 'known_app_hang' } else { 'known_app_crash' }
        $signals += [ordered]@{
            type = $sigType; severity = 'warning'; value = $null; threshold = $null
            app_id = $matchedApp.id
            message = "$($matchedApp.label): $($rr.record_type) - $($rr.product)"
            recommendation = $matchedApp.remediation
            observed_at = $rr.time
            event_key = "known|$($matchedApp.id)|$($rr.time)|$($rr.event_id)"
        }
    }
}

foreach ($rr in $reliabilityRecords | Where-Object { $_.record_type_id -in 1, 2, 4, 10 }) {
    $prodLc = ($rr.product + ' ' + $rr.source).ToLower()
    if (Find-HeavyApp -Text $prodLc -Apps $knownHeavyApps) { continue }

    $sev = if ($rr.record_type_id -eq 4) { 'critical' } else { 'warning' }
    $type = switch ($rr.record_type_id) {
        1  { 'app_crash' }
        10 { 'app_hang' }
        4  { 'bugcheck' }
        default { 'system_failure' }
    }
    $rec = if ($rr.record_type_id -in 1, 10) { $recs.app_crash } else { $recs.system_failure }
    $signals += [ordered]@{
        type = $type; severity = $sev; value = $null; threshold = $null
        message = "$($rr.record_type): $($rr.product) - $($rr.message)"
        recommendation = $rec
        observed_at = $rr.time
        event_key   = "$($rr.time)|$($rr.event_id)|$($rr.product)"
    }
}

# --- Linha do tempo do dia (performance: disco, temperatura, sinais) ---
$timelineToday = @()
foreach ($sig in $signals) {
    $t = if ($sig.observed_at) { $sig.observed_at } else { $collectedAt }
    try {
        if ([datetimeoffset]::Parse($t).DateTime -ge $todayStart) {
            $timelineToday += [ordered]@{
                time     = $t
                hour     = [datetimeoffset]::Parse($t).DateTime.Hour
                category = $sig.type
                severity = $sig.severity
                title    = $sig.message
                detail   = $sig.recommendation
                value    = $sig.value
                threshold = $sig.threshold
                app_id   = $sig.app_id
            }
        }
    } catch {}
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

# --- Fila de reparo automatico (tarefa oculta BelarcInventoryRepair) ---
$repairReasons = @()
$autoRepairTypes = @(
    'known_app_hang', 'known_app_crash', 'erp_unreachable', 'disk_saturation',
    'disk_critical', 'disk_low', 'temp_critical', 'app_hang', 'app_crash'
)
foreach ($sig in $signals) {
    if (($sig.type -in $autoRepairTypes) -or ($sig.severity -eq 'critical')) {
        if ($repairReasons -notcontains $sig.type) { $repairReasons += $sig.type }
    }
}
$autoRepairQueued = $false
if ($repairReasons.Count -gt 0) {
    $repairDataDir = Join-Path $env:ProgramData 'BelarcInventory'
    New-Item -ItemType Directory -Force -Path $repairDataDir | Out-Null
    $pendingPath = Join-Path $repairDataDir 'repair-pending.json'
    $repairProfile = if ($repairReasons | Where-Object { $_ -match 'known_app|erp_|app_' }) { 'AppsCriticos' }
        elseif ($repairReasons | Where-Object { $_ -match 'disk_critical|disk_low' }) { 'Completo' }
        else { 'Rapido' }
    if (-not (Test-Path $pendingPath)) {
        [ordered]@{
            queued_at = (Get-Date).ToString('o')
            reasons = $repairReasons
            profile = $repairProfile
            signal_messages = @($signals | Where-Object { $_.type -in $repairReasons } | ForEach-Object { $_.message })
            hostname = $env:COMPUTERNAME
        } | ConvertTo-Json -Compress -Depth 5 | Set-Content $pendingPath -Encoding UTF8
        $autoRepairQueued = $true
    }
}

$result = [ordered]@{
    collected_at         = $collectedAt
    cpu                  = [ordered]@{ load_percent = $cpuLoad; logical_cores = $cpuCores }
    memory               = [ordered]@{
        total_gb     = $memTotalGb
        free_gb      = $memFreeGb
        used_percent = $memUsedPct
    }
    logical_disks        = $logicalDisks
    physical_disk_io     = [ordered]@{
        total_disk_time_percent = $diskTimeTotal
        avg_queue_length        = $diskQueue
        per_disk                = $physicalDisks
        disk_time_samples       = $diskTimeSamples
        disk_queue_samples      = $diskQueueSamples
        peak_disk_time_percent  = $diskTimeTotal
    }
    temperatures         = $temperatures
    top_processes        = [ordered]@{ by_cpu = $topCpu; by_memory = $topMem }
    reliability_records  = $reliabilityRecords
    erp_server           = [ordered]@{
        ip         = $erpServerIp
        reachable  = $erpReachable
        latency_ms = $erpLatencyMs
    }
    known_heavy_apps     = $knownAppStatus
    signals              = $signals
    daily_timeline       = [ordered]@{
        date        = (Get-Date).ToString('yyyy-MM-dd')
        event_count = $timelineToday.Count
        events      = $timelineToday
        by_hour     = $hourlyBuckets
    }
    summary              = [ordered]@{
        signal_count            = $signals.Count
        reliability_failures    = @($reliabilityRecords | Where-Object { $_.record_type_id -in 1, 2, 4 }).Count
        max_temperature_celsius = ($temperatures | ForEach-Object { $_.celsius } | Measure-Object -Maximum).Maximum
        worst_disk_free_percent = ($logicalDisks | ForEach-Object { $_.free_percent } | Measure-Object -Minimum).Minimum
        peak_disk_time_percent  = $diskTimeTotal
        erp_reachable           = $erpReachable
        known_app_issues        = @($signals | Where-Object { $_.type -like 'known_*' -or $_.type -eq 'erp_unreachable' }).Count
        timeline_today_count    = $timelineToday.Count
        auto_repair_queued      = $autoRepairQueued
        auto_repair_reasons     = $repairReasons
    }
}

$result | ConvertTo-Json -Compress -Depth 10
