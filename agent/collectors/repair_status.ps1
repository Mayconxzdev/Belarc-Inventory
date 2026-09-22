$ErrorActionPreference = 'SilentlyContinue'

$DataDir = Join-Path $env:ProgramData 'BelarcInventory'
$PendingFile = Join-Path $DataDir 'repair-pending.json'
$ComparisonLog = Join-Path $DataDir 'repair-comparison.jsonl'
$RunsFile = Join-Path $DataDir 'repair-runs.json'
$LastFile = Join-Path $DataDir 'repair-last.json'
$RepairLog = Join-Path $DataDir 'repair-auto.log'
$MaintLog = Join-Path $DataDir 'maintenance.log'

$pending = $null
if (Test-Path $PendingFile) {
    try { $pending = Get-Content $PendingFile -Raw | ConvertFrom-Json } catch {}
}

$comparisons = @()
if (Test-Path $ComparisonLog) {
    Get-Content $ComparisonLog -Tail 15 -ErrorAction SilentlyContinue | ForEach-Object {
        try { $comparisons += ($_ | ConvertFrom-Json) } catch {}
    }
}

$runs = @()
if (Test-Path $RunsFile) {
    try { $runs = @(Get-Content $RunsFile -Raw | ConvertFrom-Json) } catch {}
}

$last = $null
if (Test-Path $LastFile) {
    try { $last = Get-Content $LastFile -Raw | ConvertFrom-Json } catch {}
}

$taskExists = $false
$taskNext = $null
try {
    $t = Get-ScheduledTask -TaskName 'BelarcInventoryRepair' -ErrorAction Stop
    $taskExists = $true
    $info = Get-ScheduledTaskInfo -TaskName 'BelarcInventoryRepair' -ErrorAction SilentlyContinue
    if ($info -and $info.NextRunTime) { $taskNext = $info.NextRunTime.ToString('o') }
} catch {}

$repairLogTail = @()
if (Test-Path $RepairLog) { $repairLogTail = @(Get-Content $RepairLog -Tail 20) }
$maintLogTail = @()
if (Test-Path $MaintLog) { $maintLogTail = @(Get-Content $MaintLog -Tail 15) }

$result = [ordered]@{
    collected_at = (Get-Date).ToString('o')
    pending = $pending
    has_pending = ($null -ne $pending)
    scheduled_task_installed = $taskExists
    scheduled_task_next_run = $taskNext
    last_repair = $last
    comparisons = @($comparisons)
    recent_runs = @($runs | Select-Object -First 10)
    repair_log_tail = $repairLogTail
    maintenance_log_tail = $maintLogTail
    summary = [ordered]@{
        total_repairs = $comparisons.Count
        last_improved = if ($last) { $last.improved } else { $null }
        last_completed = if ($last) { $last.last_completed } else { $null }
    }
}

$result | ConvertTo-Json -Compress -Depth 10
