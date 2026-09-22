# Programas corporativos monitorados para travamentos / falhas (compartilhado entre coletores).

function Get-HeavyAppsConfig {
    param([string]$BaseDir = $PSScriptRoot)

    . (Join-Path $BaseDir '_load-profile.ps1')
    $profile = Get-CompanyProfile -BaseDir $BaseDir
    $erpIp = $profile.erp_server_ip
    $erpName = $profile.erp_name

    $apps = @()
    foreach ($branch in @($profile.erp_branches)) {
        $procs = @($branch.process_patterns)
        if (-not $procs.Count) { $procs = @('sistema') }
        $products = @($branch.product_patterns)
        if (-not $products.Count) { $products = @('sistema.exe', 'cybersul', 'simnext') }
        $installDir = $branch.install_path
        if ($installDir -match '\\[^\\]+$') { $installDir = $installDir -replace '\\[^\\]+$', '' }
        $apps += [ordered]@{
            id               = $branch.id
            label            = $branch.label
            procs            = $procs
            products         = $products
            path             = $branch.install_path
            remediation      = "Ping $erpIp, reiniciar cliente $erpName, executar executar-manutencao.ps1 -Perfil AppsCriticos"
        }
    }

    $apps += @(
        [ordered]@{
            id = 'fusion360'; label = 'Fusion 360'
            procs = @('fusion', 'fusion360', 'adsk')
            products = @('fusion', 'autodesk fusion', 'fusion360', 'fusion.exe')
            path = $null
            remediation = 'Limpar cache Autodesk webdeploy, atualizar GPU, winget upgrade Autodesk.Fusion360'
        },
        [ordered]@{
            id = 'autocad'; label = 'AutoCAD'
            procs = @('acad', 'autocad')
            products = @('autocad', 'acad.exe', 'acadlt')
            path = $null
            remediation = 'PURGE/AUDIT no DWG, limpar temp ACAD, desativar aceleracao hardware se travar'
        },
        [ordered]@{
            id = 'libreoffice'; label = 'LibreOffice'
            procs = @('soffice', 'libreoffice')
            products = @('libreoffice', 'soffice.bin', 'soffice.exe', 'writer', 'calc', 'impress')
            path = $null
            remediation = 'Encerrar soffice.bin, limpar cache LibreOffice, winget upgrade LibreOffice'
        },
        [ordered]@{
            id = 'libredraw'; label = 'LibreOffice Draw'
            procs = @('soffice', 'libreoffice')
            products = @('libreoffice draw', 'draw', 'soffice.bin')
            path = $null
            remediation = 'Encerrar soffice.bin, limpar cache Draw, abrir arquivo em modo seguro'
        }
    )

    [ordered]@{
        profile   = $profile
        erp_ip    = $erpIp
        erp_name  = $erpName
        apps      = $apps
    }
}

function Test-HeavyAppMatch {
    param(
        [string]$Text,
        [object]$App
    )
    $t = ($Text + '').ToLower()
    foreach ($pn in @($App.procs)) {
        if ($t -like "*$pn*") { return $true }
    }
    foreach ($pp in @($App.products)) {
        if ($t -like "*$pp*") { return $true }
    }
    if ($App.path -and $t -like "*$($App.path.ToLower())*") { return $true }
    $false
}

function Find-HeavyApp {
    param(
        [string]$Text,
        [array]$Apps
    )
    foreach ($app in $Apps) {
        if (Test-HeavyAppMatch -Text $Text -App $app) { return $app }
    }
    $null
}
