# Smoke test isolado do Belarc Inventory.
# Nao instala servico, nao cria tarefas e nao acessa o NAS real.
# Usa o mesmo belarc-server.exe, as mesmas rotas e o mesmo fluxo de tickets.

[CmdletBinding()]
param(
    [int]$Port = 18080,
    [switch]$RunAgent,
    [switch]$Build,
    [string]$DataRoot = '',
    [int]$AgentTimeoutSeconds = 60
)

$ErrorActionPreference = 'Stop'
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

if (-not $DataRoot) {
    $DataRoot = Join-Path $RepoRoot ('test-data\isolated\' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
}
$DataRoot = [IO.Path]::GetFullPath($DataRoot)
$NasRoot = Join-Path $DataRoot 'nas\Chamados'
$ProfilePath = Join-Path $DataRoot 'company-profile.test.json'
$ServerLog = Join-Path $DataRoot 'server.stdout.log'
$ServerErrorLog = Join-Path $DataRoot 'server.stderr.log'
$BaseUrl = "http://127.0.0.1:$Port"

New-Item -ItemType Directory -Path $DataRoot -Force | Out-Null
New-Item -ItemType Directory -Path $NasRoot -Force | Out-Null

if (Get-Command Get-NetTCPConnection -ErrorAction SilentlyContinue) {
    $occupied = @(Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue)
    if ($occupied.Count -gt 0) {
        throw "A porta $Port ja esta em uso. Escolha outra porta; nenhum processo existente sera encerrado."
    }
}

$profile = [ordered]@{
    company_name = 'Belarc Isolated Test'
    project_tagline = 'Perfil fictício para homologação local'
    erp_name = 'ERP-Test'
    erp_server_ip = '127.0.0.1'
    inventory_server_ip = '127.0.0.1'
    antivirus_name = 'ESET-Test'
    banking_app = [ordered]@{ id = 'bank-test'; label = 'Bank Test' }
    erp_branches = @(
        [ordered]@{
            id = 'erp-test'
            label = 'ERP Test'
            install_path = 'C:\IsolatedTest\erp.exe'
            temp_paths = @('C:\IsolatedTest\temp')
            process_patterns = @('erp-test')
            product_patterns = @('erp-test.exe')
        }
    )
    ui = [ordered]@{
        erp_user = 'ERP Test - usuario'
        erp_password = 'ERP Test - senha'
        erp_offline_warning = 'ERP Test indisponivel no ambiente isolado'
        freeze_prone_description = 'Aplicacoes ficticias do ambiente isolado'
        critical_apps_summary = 'ERP-Test'
    }
}
$profile | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $ProfilePath -Encoding UTF8

$serverExe = Join-Path $RepoRoot 'target\debug\belarc-server.exe'
if (-not (Test-Path -LiteralPath $serverExe)) {
    $serverExe = Join-Path $RepoRoot 'target\release\belarc-server.exe'
}
if (-not (Test-Path -LiteralPath $serverExe) -and $Build) {
    Push-Location $RepoRoot
    try {
        cargo build -p belarc-server --bin belarc-server
        if ($LASTEXITCODE -ne 0) { throw "cargo build do servidor falhou: exit $LASTEXITCODE" }
    } finally {
        Pop-Location
    }
    $serverExe = Join-Path $RepoRoot 'target\debug\belarc-server.exe'
}
if (-not (Test-Path -LiteralPath $serverExe)) {
    throw "belarc-server.exe nao encontrado. Execute com -Build ou compile cargo build -p belarc-server --bin belarc-server."
}

$agentExe = Join-Path $RepoRoot 'target\debug\belarc-agent.exe'
if (-not (Test-Path -LiteralPath $agentExe)) {
    $agentExe = Join-Path $RepoRoot 'target\release\belarc-agent.exe'
}
if ($RunAgent -and -not (Test-Path -LiteralPath $agentExe) -and $Build) {
    Push-Location $RepoRoot
    try {
        cargo build -p belarc-agent --bin belarc-agent
        if ($LASTEXITCODE -ne 0) { throw "cargo build do agente falhou: exit $LASTEXITCODE" }
    } finally {
        Pop-Location
    }
    $agentExe = Join-Path $RepoRoot 'target\debug\belarc-agent.exe'
}
if ($RunAgent -and -not (Test-Path -LiteralPath $agentExe)) {
    throw "belarc-agent.exe nao encontrado. Execute com -Build ou compile cargo build -p belarc-agent --bin belarc-agent."
}

$testTiUser = 'ti-isolated'
$testTiPassword = 'Isolated-Test-Only-Change-Me-2026!'
$testAgentToken = 'isolated-agent-token-2026'
$testHostname = 'ISOLATED-PC-01'
$serverProcess = $null
$agentProcess = $null
$savedEnv = @{}
foreach ($name in @('BELARC_DATA_DIR', 'BELARC_LISTEN', 'BELARC_CHAMADOS_ROOT', 'BELARC_COMPANY_PROFILE', 'BELARC_TI_USER', 'BELARC_TI_PASSWORD', 'BELARC_SERVER_URL', 'BELARC_AGENT_TOKEN', 'PROGRAMDATA')) {
    $savedEnv[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}

function Restore-TestEnvironment {
    foreach ($name in $savedEnv.Keys) {
        [Environment]::SetEnvironmentVariable($name, $savedEnv[$name], 'Process')
    }
}

function Invoke-TestJson {
    param(
        [Parameter(Mandatory)] [ValidateSet('GET', 'POST', 'PATCH', 'DELETE')] [string]$Method,
        [Parameter(Mandatory)] [string]$Path,
        [hashtable]$Headers = @{},
        $Body = $null
    )
    $params = @{
        Uri = ($BaseUrl.TrimEnd('/') + $Path)
        Method = $Method
        Headers = $Headers
        TimeoutSec = 30
    }
    if ($null -ne $Body) {
        $params.ContentType = 'application/json'
        $params.Body = ($Body | ConvertTo-Json -Depth 12 -Compress)
    }
    Invoke-RestMethod @params
}

function Wait-Server {
    param([int]$TimeoutSeconds = 30)
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        try {
            $health = Invoke-RestMethod ($BaseUrl + '/api/health') -TimeoutSec 2
            if ($health.status -eq 'ok') { return $health }
        } catch {
            Start-Sleep -Milliseconds 500
        }
    } while ((Get-Date) -lt $deadline)
    throw "Servidor isolado nao respondeu em $BaseUrl. Consulte $ServerLog e $ServerErrorLog."
}

function Stop-ProcessTree {
    param([int]$RootPid)
    $children = @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object { $_.ParentProcessId -eq $RootPid })
    foreach ($child in $children) {
        Stop-ProcessTree -RootPid ([int]$child.ProcessId)
    }
    $target = Get-Process -Id $RootPid -ErrorAction SilentlyContinue
    if ($target) {
        Stop-Process -Id $RootPid -Force -ErrorAction SilentlyContinue
    }
}

try {
    [Environment]::SetEnvironmentVariable('BELARC_DATA_DIR', $DataRoot, 'Process')
    [Environment]::SetEnvironmentVariable('BELARC_LISTEN', "127.0.0.1:$Port", 'Process')
    [Environment]::SetEnvironmentVariable('BELARC_CHAMADOS_ROOT', $NasRoot, 'Process')
    [Environment]::SetEnvironmentVariable('BELARC_COMPANY_PROFILE', $ProfilePath, 'Process')
    [Environment]::SetEnvironmentVariable('BELARC_TI_USER', $testTiUser, 'Process')
    [Environment]::SetEnvironmentVariable('BELARC_TI_PASSWORD', $testTiPassword, 'Process')

    Write-Host "=== Belarc isolated smoke test ===" -ForegroundColor Cyan
    Write-Host "Servidor: $serverExe"
    Write-Host "Dados:    $DataRoot"
    Write-Host "NAS local: $NasRoot"
    Write-Host "URL:      $BaseUrl"

    $serverProcess = Start-Process `
        -FilePath $serverExe `
        -WorkingDirectory $RepoRoot `
        -RedirectStandardOutput $ServerLog `
        -RedirectStandardError $ServerErrorLog `
        -PassThru `
        -WindowStyle Hidden

    $null = Wait-Server
    Write-Host '[PASS] health' -ForegroundColor Green
    $portalPage = Invoke-WebRequest ($BaseUrl + '/cliente.html') -UseBasicParsing -TimeoutSec 10
    if ($portalPage.StatusCode -ne 200 -or $portalPage.Content -notmatch 'spellcheck="true"') {
        throw 'Pagina cliente.html nao foi servida com o corretor ortografico habilitado.'
    }
    $portalManifest = Invoke-WebRequest ($BaseUrl + '/cliente.webmanifest') -UseBasicParsing -TimeoutSec 10
    $portalWorker = Invoke-WebRequest ($BaseUrl + '/cliente-sw.js') -UseBasicParsing -TimeoutSec 10
    $portalScript = Invoke-WebRequest ($BaseUrl + '/cliente.js') -UseBasicParsing -TimeoutSec 10
    if ($portalManifest.StatusCode -ne 200 -or $portalWorker.StatusCode -ne 200 -or $portalScript.StatusCode -ne 200) {
        throw "Recursos PWA/atualizacao do portal nao foram servidos corretamente. manifest=$($portalManifest.StatusCode) worker=$($portalWorker.StatusCode) script=$($portalScript.StatusCode)"
    }
    Write-Host '[PASS] portal web, corretor pt-BR e recursos PWA' -ForegroundColor Green

    $login = Invoke-TestJson -Method POST -Path '/api/auth/login' -Body @{ username = $testTiUser; password = $testTiPassword }
    if (-not $login.token) { throw 'Login isolado nao retornou sessao.' }
    $auth = @{ Authorization = "Bearer $($login.token)" }
    Write-Host '[PASS] login TI isolado' -ForegroundColor Green

    $register = Invoke-TestJson -Method POST -Path '/api/register' -Body @{
        agent_token = $testAgentToken
        hostname = $testHostname
        serial = 'ISOLATED-SERIAL-01'
        machine_uuid = '00000000-0000-4000-8000-000000000001'
        mac_primary = '02:00:00:00:00:01'
    }
    if (-not $register.machine_id) { throw 'Registro isolado nao retornou machine_id.' }
    $machineId = $register.machine_id
    Write-Host "[PASS] register machine_id=$machineId" -ForegroundColor Green

    $null = Invoke-TestJson -Method POST -Path '/api/heartbeat' -Body @{
        agent_token = $testAgentToken
        hostname = $testHostname
        serial = 'ISOLATED-SERIAL-01'
        uuid = '00000000-0000-4000-8000-000000000001'
        mac_primary = '02:00:00:00:00:01'
        logged_user = 'isolated.user'
        ip_address = '127.0.0.1'
        uptime_seconds = 123
        last_boot = $null
    }
    Write-Host '[PASS] heartbeat' -ForegroundColor Green

    $inventory = @{
        agent_token = $testAgentToken
        hostname = $testHostname
        tier = 't1'
        collected_at = (Get-Date).ToUniversalTime().ToString('o')
        collectors = @(
            @{ name = 'identity'; version = 'isolated-test'; data = @{ hostname = $testHostname; machine_uuid = '00000000-0000-4000-8000-000000000001'; serial = 'ISOLATED-SERIAL-01'; mac_primary = '02:00:00:00:00:01'; logged_user = 'isolated.user'; ip_primary = '127.0.0.1' }; hash = ('1' * 64); duration_ms = 1; error = $null },
            @{ name = 'hardware'; version = 'isolated-test'; data = @{ model = 'Isolated Test PC'; cpu = 'Test CPU'; ram_gb = 16; disks = @(@{ letter = 'C:'; size_gb = 256; free_gb = 128 }) }; hash = ('2' * 64); duration_ms = 1; error = $null },
            @{ name = 'software'; version = 'isolated-test'; data = @{ standard_apps = @(@{ id = 'erp-test'; label = 'ERP Test'; category = 'erp'; installed = $true }, @{ id = 'eset'; label = 'ESET-Test'; category = 'seguranca'; installed = $true; version = '1.0' }) }; hash = ('3' * 64); duration_ms = 1; error = $null },
            @{ name = 'performance'; version = 'isolated-test'; data = @{ cpu_load_percent = 24.5; memory_used_percent = 62.0; erp_server = @{ ip = '127.0.0.1'; reachable = $true; latency_ms = 1.0 } }; hash = ('4' * 64); duration_ms = 1; error = $null }
        )
    }
    $inventoryResult = Invoke-TestJson -Method POST -Path '/api/inventory' -Body $inventory
    if ($null -eq $inventoryResult.health_score) { throw 'Inventory isolado nao retornou health_score.' }
    Write-Host "[PASS] inventory score=$($inventoryResult.health_score) changed=$($inventoryResult.changed_collectors)" -ForegroundColor Green

    $machines = @(Invoke-TestJson -Method GET -Path '/api/machines' -Headers $auth)
    if (-not ($machines | Where-Object { $_.id -eq $machineId })) { throw 'Maquina isolada nao apareceu em /api/machines.' }
    $null = Invoke-TestJson -Method GET -Path '/api/dashboard' -Headers $auth
    $null = Invoke-TestJson -Method GET -Path "/api/machines/$machineId" -Headers $auth
    $tiSession = Invoke-TestJson -Method GET -Path '/api/auth/me' -Headers $auth
    if (-not $tiSession.authenticated) { throw 'Sessao TI perdeu autenticacao antes do provisionamento do portal.' }
    $machinePortalConfig = Invoke-TestJson -Method PATCH -Path "/api/machines/$machineId/ticket-routing" -Headers $auth -Body @{
        receive_departments = @('projeto', 'producao')
    }
    if (-not ($machinePortalConfig.ticket_receive_departments -contains 'projeto') -or -not ($machinePortalConfig.ticket_receive_departments -contains 'producao')) { throw 'TI nao persistiu os setores recebidos pelo PC.' }
    Write-Host '[PASS] dashboard e detalhe da maquina' -ForegroundColor Green

    # Fluxo definitivo de PC corporativo: agente prova a maquina; o navegador
    # recebe sessao curta do portal, sem senha e sem token do agente.
    $agentAuth = @{ Authorization = "Bearer $testAgentToken" }
    $autoPortal = Invoke-TestJson -Method POST -Path '/api/portal/device-session' -Headers $agentAuth -Body @{ mode = 'portal' }
    if (-not $autoPortal.token) { throw 'Sessao automatica do portal nao foi emitida.' }
    $autoPortalAuth = @{ Authorization = "Bearer $($autoPortal.token)" }
    $autoPortalMe = Invoke-TestJson -Method GET -Path '/api/portal/auth/me' -Headers $autoPortalAuth
    $autoPortalMachines = @(Invoke-TestJson -Method GET -Path '/api/portal/machines' -Headers $autoPortalAuth)
    if (-not $autoPortalMe.authenticated -or -not $autoPortalMe.can_open_ticket -or -not ($autoPortalMe.receive_departments -contains 'projeto') -or -not ($autoPortalMe.receive_departments -contains 'producao') -or -not ($autoPortalMachines | Where-Object { $_.id -eq $machineId })) { throw 'Portal automatico nao recebeu o PC e os setores corretos.' }
    $projectReceived = Invoke-TestJson -Method POST -Path '/api/tickets' -Headers $auth -Body @{ title = 'Recebimento Projeto'; machine_id = $machineId; department = 'projeto' }
    $productionReceived = Invoke-TestJson -Method POST -Path '/api/tickets' -Headers $auth -Body @{ title = 'Recebimento Produção'; machine_id = $machineId; department = 'producao' }
    $drawingHidden = Invoke-TestJson -Method POST -Path '/api/tickets' -Headers $auth -Body @{ title = 'Não deve chegar em Desenho'; machine_id = $machineId; department = 'desenho' }
    $autoReceivedQueue = @(Invoke-TestJson -Method GET -Path '/api/tickets' -Headers $autoPortalAuth)
    if (-not ($autoReceivedQueue | Where-Object { $_.id -eq $projectReceived.id }) -or -not ($autoReceivedQueue | Where-Object { $_.id -eq $productionReceived.id }) -or ($autoReceivedQueue | Where-Object { $_.id -eq $drawingHidden.id })) { throw 'Fila automática não respeitou Projeto/Produção e bloqueio de Desenho.' }
    Write-Host '[PASS] sessao automatica: mesmo PC abre e recebe Projeto/Produção' -ForegroundColor Green

    # Parte 1 — portal separado, contas locais, vinculo usuario-PC e fila por setor.
    # Nenhuma credencial ou host de producao entra neste teste.
    Write-Host '[INFO] validando provisionamento de contas do portal' -ForegroundColor DarkCyan
    $requester = Invoke-TestJson -Method POST -Path '/api/portal/users' -Headers $auth -Body @{
        username = 'usuario-isolado'; display_name = 'Usuário Isolado'; password = 'Portal-Test-Only-2026!'; role = 'requester'
    }
    Write-Host '[INFO] solicitante criado; vinculando PC' -ForegroundColor DarkCyan
    $null = Invoke-TestJson -Method POST -Path "/api/portal/users/$($requester.id)/machines/$machineId" -Headers $auth
    Write-Host '[INFO] PC vinculado; criando atendente de setor' -ForegroundColor DarkCyan
    $sector = Invoke-TestJson -Method POST -Path '/api/portal/users' -Headers $auth -Body @{
        username = 'producao-isolado'; display_name = 'Produção Isolada'; password = 'Portal-Test-Only-2026!'; role = 'sector_agent'; department = 'producao'
    }
    Write-Host '[INFO] atendente criado; autenticando solicitante' -ForegroundColor DarkCyan
    $portalLogin = Invoke-TestJson -Method POST -Path '/api/portal/auth/login' -Body @{ username = 'usuario-isolado'; password = 'Portal-Test-Only-2026!' }
    if (-not $portalLogin.token) { throw 'Login do portal nao retornou sessao.' }
    $portalAuth = @{ Authorization = "Bearer $($portalLogin.token)" }
    Write-Host '[INFO] solicitante autenticado; lendo PC vinculado' -ForegroundColor DarkCyan
    $portalMachines = @(Invoke-TestJson -Method GET -Path '/api/portal/machines' -Headers $portalAuth)
    if (-not ($portalMachines | Where-Object { $_.id -eq $machineId })) { throw 'Portal nao retornou apenas o PC vinculado.' }
    $sectorTicket = Invoke-TestJson -Method POST -Path '/api/tickets' -Headers $portalAuth -Body @{
        title = 'Teste portal para producao'; description = 'Texto com corretor ortografico no cliente web.'; priority = 'normal'; department = 'producao'; machine_id = $machineId
        attachments = @(@{ filename = 'foto-teste.txt'; content_base64 = 'Zm90by1pc29sYWRh' })
    }
    if ($sectorTicket.department -ne 'producao' -or -not $sectorTicket.requester_user_id -or @($sectorTicket.attachments).Count -ne 1) { throw 'Chamado do portal nao persistiu setor/solicitante/anexo.' }
    $sectorTicket = Invoke-TestJson -Method POST -Path "/api/tickets/$($sectorTicket.id)/comments" -Headers $portalAuth -Body @{ body = 'Comentário enviado pelo solicitante no portal móvel.' }
    if (-not (@($sectorTicket.comments) | Where-Object { $_.body -match 'portal móvel' })) { throw 'Comentário do solicitante não foi persistido.' }
    $sectorLogin = Invoke-TestJson -Method POST -Path '/api/portal/auth/login' -Body @{ username = 'producao-isolado'; password = 'Portal-Test-Only-2026!' }
    $sectorAuth = @{ Authorization = "Bearer $($sectorLogin.token)" }
    $sectorQueue = @(Invoke-TestJson -Method GET -Path '/api/tickets' -Headers $sectorAuth)
    if (-not ($sectorQueue | Where-Object { $_.id -eq $sectorTicket.id })) { throw 'Atendente do setor nao recebeu o chamado da propria fila.' }
    $sectorClosed = Invoke-TestJson -Method POST -Path "/api/tickets/$($sectorTicket.id)/close" -Headers $sectorAuth -Body @{ resolution = 'Atendimento de Produção isolado concluído.' }
    if ($sectorClosed.status -ne 'done') { throw 'Atendente do setor nao conseguiu concluir o chamado da propria fila.' }
    $requesterQueue = @(Invoke-TestJson -Method GET -Path '/api/tickets' -Headers $portalAuth)
    if (-not ($requesterQueue | Where-Object { $_.id -eq $sectorTicket.id })) { throw 'Solicitante nao recebeu o proprio chamado.' }
    Write-Host '[PASS] portal: conta, vinculo PC, setor e fila restrita' -ForegroundColor Green

    $ticket = Invoke-TestJson -Method POST -Path '/api/tickets' -Headers $auth -Body @{
        title = 'Teste isolado de chamado'
        description = 'Chamado temporario do ambiente local de homologacao.'
        priority = 'low'
        machine_id = $machineId
    }
    if (-not $ticket.id) { throw 'Criacao de chamado isolado nao retornou id.' }
    $ticketId = $ticket.id
    Write-Host "[PASS] ticket criado code=$($ticket.code)" -ForegroundColor Green

    $null = Invoke-TestJson -Method POST -Path "/api/tickets/$ticketId/comments" -Headers $auth -Body @{ body = 'Comentario do smoke test'; author_name = 'TI isolado' }
    $ticketAfterChecklist = Invoke-TestJson -Method POST -Path "/api/tickets/$ticketId/checklist" -Headers $auth -Body @{ label = 'Validar fluxo isolado' }
    $checklistId = @($ticketAfterChecklist.checklist)[-1].id
    if ($checklistId) {
        $null = Invoke-TestJson -Method PATCH -Path "/api/tickets/$ticketId/checklist/$checklistId" -Headers $auth -Body @{ done = $true }
    }
    $null = Invoke-TestJson -Method PATCH -Path "/api/tickets/$ticketId" -Headers $auth -Body @{ status = 'in_progress' }
    $closed = Invoke-TestJson -Method POST -Path "/api/tickets/$ticketId/close" -Headers $auth -Body @{ resolution = 'Fluxo isolado validado' }
    if ($closed.status -ne 'done') { throw "Chamado isolado nao fechou; status=$($closed.status)" }
    Write-Host '[PASS] comentario, checklist, atualizacao e fechamento de chamado' -ForegroundColor Green

    $null = Invoke-TestJson -Method GET -Path '/api/tickets' -Headers $auth
    $null = Invoke-TestJson -Method GET -Path '/api/tickets/stats' -Headers $auth
    Write-Host '[PASS] lista e estatisticas de chamados' -ForegroundColor Green

    if ($RunAgent) {
        [Environment]::SetEnvironmentVariable('BELARC_SERVER_URL', $BaseUrl, 'Process')
        [Environment]::SetEnvironmentVariable('BELARC_AGENT_TOKEN', 'isolated-real-agent-token-2026', 'Process')
        [Environment]::SetEnvironmentVariable('PROGRAMDATA', (Join-Path $DataRoot 'programdata'), 'Process')
        New-Item -ItemType Directory -Path $env:PROGRAMDATA -Force | Out-Null
        $agentOut = Join-Path $DataRoot 'agent.stdout.log'
        $agentErr = Join-Path $DataRoot 'agent.stderr.log'
        $agentProcess = Start-Process `
            -FilePath $agentExe `
            -ArgumentList @('collect', 't1', '--force') `
            -WorkingDirectory $RepoRoot `
            -RedirectStandardOutput $agentOut `
            -RedirectStandardError $agentErr `
            -PassThru `
            -WindowStyle Hidden
        $agentDeadline = (Get-Date).AddSeconds($AgentTimeoutSeconds)
        while (-not $agentProcess.HasExited -and (Get-Date) -lt $agentDeadline) {
            Start-Sleep -Milliseconds 500
        }
        if (-not $agentProcess.HasExited) {
            Stop-ProcessTree -RootPid $agentProcess.Id
            throw "Agente foreground isolado excedeu ${AgentTimeoutSeconds}s; collector provavelmente bloqueado. Consulte $agentOut e $agentErr."
        }
        if ($agentProcess.ExitCode -ne 0) { throw "Agente foreground isolado falhou: exit $($agentProcess.ExitCode). Consulte $agentOut e $agentErr." }
        $agentMachines = @(Invoke-TestJson -Method GET -Path '/api/machines' -Headers $auth)
        if (-not ($agentMachines | Where-Object { $_.hostname -ne $testHostname })) { throw 'O agente executou, mas nao apareceu uma maquina adicional no servidor isolado.' }
        Write-Host '[PASS] agente foreground e collectors locais' -ForegroundColor Green
    }

    Write-Host ''
    Write-Host "RESULTADO: PASS" -ForegroundColor Green
    Write-Host "Dados de evidencia preservados em: $DataRoot"
    Write-Host 'O chamado de teste foi mantido somente no banco/NAS local desta execução.'
} catch {
    Write-Host ''
    Write-Host "RESULTADO: FAIL - $($_.Exception.Message)" -ForegroundColor Red
    Write-Host "Dados de evidencia preservados em: $DataRoot" -ForegroundColor Yellow
    if (Test-Path -LiteralPath $ServerErrorLog) { Write-Host "Log stderr: $ServerErrorLog" -ForegroundColor Yellow }
    exit 1
} finally {
    if ($agentProcess -and -not $agentProcess.HasExited) {
        Stop-ProcessTree -RootPid $agentProcess.Id
    }
    if ($serverProcess -and -not $serverProcess.HasExited) {
        Stop-ProcessTree -RootPid $serverProcess.Id
    }
    Restore-TestEnvironment
}
