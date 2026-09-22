# Carrega config/company-profile.json para coletores e scripts de manutencao.
function Get-CompanyProfile {
    param([string]$BaseDir = $PSScriptRoot)

    $candidates = @(
        (Join-Path $BaseDir '_company-profile.json'),
        (Join-Path $BaseDir '..\config\company-profile.json'),
        (Join-Path $BaseDir '..\..\config\company-profile.json'),
        (Join-Path $BaseDir '..\..\..\config\company-profile.json')
    )

    foreach ($path in $candidates) {
        try {
            $resolved = [System.IO.Path]::GetFullPath($path)
            if (Test-Path $resolved) {
                return Get-Content $resolved -Raw -Encoding UTF8 | ConvertFrom-Json
            }
        } catch { }
    }

    [pscustomobject]@{
        company_name          = 'Demo Corp'
        erp_name              = 'AcmeERP'
        erp_server_ip         = '10.0.0.50'
        inventory_server_ip   = '10.0.0.10'
        antivirus_name        = 'ESET'
        banking_app           = [pscustomobject]@{ id = 'banking'; label = 'App Bancario Corporativo' }
        erp_branches          = @(
            [pscustomobject]@{
                id = 'erp_branch_a'; label = 'AcmeERP (Filial Alpha)'
                install_path = 'C:\AcmeERP\client.exe'
                temp_paths = @('C:\AcmeERP\temp', 'C:\AcmeERP\Temp', 'C:\AcmeERP\cache')
                process_patterns = @('sistema', 'acmeerp'); product_patterns = @('acmeerp', 'client.exe')
            },
            [pscustomobject]@{
                id = 'erp_branch_b'; label = 'AcmeERP (Filial Beta)'
                install_path = 'C:\AcmeERP_02\client.exe'
                temp_paths = @('C:\AcmeERP_02\temp', 'C:\AcmeERP_02\Temp', 'C:\AcmeERP_02\cache')
                process_patterns = @('sistema', 'acmeerp'); product_patterns = @('acmeerp', 'client.exe')
            }
        )
        ui = [pscustomobject]@{
            erp_offline_warning      = 'offline — ERP pode travar'
            critical_apps_summary    = 'ERP, Fusion 360, AutoCAD, LibreOffice Draw'
        }
    }
}
