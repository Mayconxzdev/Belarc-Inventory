[CmdletBinding()]
param(
    [string]$OutputRoot = (Join-Path $PSScriptRoot 'release')
)

$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

Write-Host '[1/4] Validando cliente desktop...' -ForegroundColor Cyan
go test .
if ($LASTEXITCODE -ne 0) { throw 'Testes Go falharam.' }
node --check frontend\dist\ticket-enhancements.js
if ($LASTEXITCODE -ne 0) { throw 'Validação JavaScript falhou.' }

Write-Host '[2/4] Compilando ChamadosServicosTI-v2.exe...' -ForegroundColor Cyan
New-Item -ItemType Directory -Path build -Force | Out-Null
go build -tags 'desktop,wv2runtime.download,production' -ldflags '-w -s -H windowsgui' -o build\ChamadosServicosTI-v2.exe .
if ($LASTEXITCODE -ne 0) { throw 'Build do cliente falhou.' }

$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$package = Join-Path $OutputRoot "ChamadosServicosTI-v2-HOMOLOGACAO-$stamp"
New-Item -ItemType Directory -Path (Join-Path $package 'build'), (Join-Path $package 'deploy') -Force | Out-Null

Write-Host '[3/4] Montando pacote adicional e reversível...' -ForegroundColor Cyan
Copy-Item -LiteralPath 'build\ChamadosServicosTI-v2.exe' -Destination (Join-Path $package 'build\ChamadosServicosTI-v2.exe') -Force
Copy-Item -LiteralPath 'deploy\Install-ChamadosServicosTI-v2.ps1' -Destination (Join-Path $package 'deploy\Install-ChamadosServicosTI-v2.ps1') -Force
Copy-Item -LiteralPath 'deploy\Rollback-ChamadosServicosTI-v2.ps1' -Destination (Join-Path $package 'deploy\Rollback-ChamadosServicosTI-v2.ps1') -Force
Copy-Item -LiteralPath 'deploy\README.md' -Destination (Join-Path $package 'LEIA-ME.md') -Force

$exe = Join-Path $package 'build\ChamadosServicosTI-v2.exe'
$hash = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
$manifest = [ordered]@{
    product = 'Chamados Serviços TI v2'
    channel = 'HOMOLOGACAO'
    generated_at = (Get-Date).ToString('o')
    executable = 'build\\ChamadosServicosTI-v2.exe'
    sha256 = $hash
    install_mode_default = 'Pilot'
    preserves_agent = $true
    preserves_legacy_executable = $true
    requires_server_compatibility = $true
}
$manifest | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $package 'release-manifest.json') -Encoding UTF8
Set-Content -LiteralPath (Join-Path $package 'SHA256.txt') -Value "$hash  build\ChamadosServicosTI-v2.exe" -Encoding ASCII

Write-Host '[4/4] Pacote pronto.' -ForegroundColor Green
Write-Host "Pasta: $package"
Write-Host "SHA-256: $hash"
Write-Host 'Use primeiro deploy\Install-ChamadosServicosTI-v2.ps1 -Mode Pilot em um PC piloto.' -ForegroundColor Yellow
