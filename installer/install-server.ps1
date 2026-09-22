# Belarc Inventory Server - Instalador
param(
    [string]$InstallDir = "$env:ProgramFiles\BelarcInventory",
    [string]$Listen = "0.0.0.0:80",
    [string]$ServerRunAs = '',
    [string]$ServerRunAsPassword = ''
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path ".\belarc-server.exe")) {
    Write-Error "belarc-server.exe not found. Build with: cargo build --release -p belarc-server"
}

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
$dataDir = "$env:ProgramData\BelarcInventoryServer"
New-Item -ItemType Directory -Force -Path $dataDir | Out-Null

[System.Environment]::SetEnvironmentVariable("BELARC_DATA_DIR", $dataDir, "Machine")
[System.Environment]::SetEnvironmentVariable("BELARC_LISTEN", $Listen, "Machine")
[System.Environment]::SetEnvironmentVariable("BELARC_CHAMADOS_ROOT", "\\FILE-SHARE\Portal\Chamados", "Machine")

Copy-Item ".\belarc-server.exe" "$InstallDir\belarc-server.exe" -Force
Copy-Item "..\server\web" "$InstallDir\web" -Recurse -Force

$taskName = "BelarcInventoryServer"
$chamadosRoot = "\\FILE-SHARE\Portal\Chamados"
$tr = "cmd /c `"set BELARC_DATA_DIR=$dataDir&& set BELARC_LISTEN=$Listen&& set BELARC_CHAMADOS_ROOT=$chamadosRoot&& `"$InstallDir\belarc-server.exe`"`""
$action = New-ScheduledTaskAction -Execute "cmd.exe" -Argument "/c `"$tr`""

if ($ServerRunAs) {
    if (-not $ServerRunAsPassword) {
        $sec = Read-Host "Senha para $ServerRunAs" -AsSecureString
        $ptr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($sec)
        try { $ServerRunAsPassword = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($ptr) }
        finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($ptr) }
    }
    $principal = New-ScheduledTaskPrincipal -UserId $ServerRunAs -LogonType Password -RunLevel Highest
    Register-ScheduledTask -TaskName $taskName -Action $action -Trigger (New-ScheduledTaskTrigger -AtStartup) -Principal $principal -Settings (New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable) -Force
    # schtasks /RP requires separate call for password on some Windows versions
    schtasks /Change /TN $taskName /RU $ServerRunAs /RP $ServerRunAsPassword 2>$null
} else {
    $principal = New-ScheduledTaskPrincipal -UserId "SYSTEM" -LogonType ServiceAccount -RunLevel Highest
    Register-ScheduledTask -TaskName $taskName -Action $action -Trigger (New-ScheduledTaskTrigger -AtStartup) -Principal $principal -Settings (New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable) -Force
}

Start-ScheduledTask -TaskName $taskName

Write-Host "Server installed. Dashboard: http://localhost"
Write-Host "Data directory: $dataDir"
Write-Host "Chamados NAS: $chamadosRoot"
if (-not $ServerRunAs) {
    Write-Host "AVISO: tarefa roda como SYSTEM — use -ServerRunAs para acesso ao NAS." -ForegroundColor Yellow
}
Write-Host ""
Write-Host "Create agent token:"
Write-Host "  Invoke-RestMethod -Method POST -Uri http://localhost/api/tokens -ContentType application/json -Body '{}'"
