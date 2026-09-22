$ErrorActionPreference = 'SilentlyContinue'

function Test-Cmd($name) {
    $c = Get-Command $name -ErrorAction SilentlyContinue
    [bool]$c
}

$docker = [ordered]@{ installed = $false; running = $false; version = $null; containers = 0 }
if (Test-Cmd 'docker') {
    $docker.installed = $true
    try {
        $ver = docker version --format '{{.Server.Version}}' 2>$null
        if ($ver) { $docker.version = $ver.Trim() }
        $ps = docker ps -q 2>$null
        if ($LASTEXITCODE -eq 0) {
            $docker.running = $true
            $docker.containers = @($ps).Count
        }
    } catch {}
}

$wsl = [ordered]@{ installed = $false; distros = @() }
if (Test-Cmd 'wsl') {
    $wsl.installed = $true
    try {
        $list = wsl -l -v 2>$null
        if ($list) {
            $wsl.distros = @($list | Select-Object -Skip 1 | ForEach-Object { $_.Trim() }) | Where-Object { $_ }
        }
    } catch {}
}

$hyperv = [ordered]@{ enabled = $false; vms = 0 }
try {
    $hv = Get-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V-All -ErrorAction SilentlyContinue
    if ($hv -and $hv.State -eq 'Enabled') {
        $hyperv.enabled = $true
        $vms = Get-VM -ErrorAction SilentlyContinue
        if ($vms) { $hyperv.vms = @($vms).Count }
    }
} catch {}

$ollama = [ordered]@{ installed = $false; running = $false; version = $null }
if (Test-Cmd 'ollama') {
    $ollama.installed = $true
    try {
        $ver = ollama --version 2>$null
        if ($ver) { $ollama.version = ($ver -replace '^ollama\s+', '').Trim() }
        $ollama.running = [bool](Get-Process ollama -ErrorAction SilentlyContinue)
    } catch {}
}

[ordered]@{
    docker  = $docker
    wsl     = $wsl
    hyperv  = $hyperv
    ollama  = $ollama
} | ConvertTo-Json -Compress -Depth 5
