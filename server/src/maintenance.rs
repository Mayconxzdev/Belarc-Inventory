use belarc_shared::CompanyProfile;

use crate::freeze_prone_apps::FreezeProneApp;

#[derive(Debug, Clone, serde::Serialize)]
pub struct MaintenanceTask {
    pub id: String,
    pub label: String,
    pub description: String,
    pub command: String,
    pub profile: String,
    pub requires_admin: bool,
    pub runs_background: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MaintenancePlaybook {
    pub erp_server_ip: String,
    pub erp_name: String,
    pub freeze_prone_apps: Vec<FreezeProneAppInfo>,
    pub profiles: Vec<MaintenanceProfile>,
    pub tasks: Vec<MaintenanceTask>,
    pub quick_start: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FreezeProneAppInfo {
    pub id: String,
    pub label: String,
    pub category: String,
    pub remediation: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MaintenanceProfile {
    pub id: String,
    pub label: String,
    pub description: String,
    pub command: String,
    pub estimated_minutes: u32,
}

pub fn maintenance_playbook(
    profile: &CompanyProfile,
    apps: &[FreezeProneApp],
) -> MaintenancePlaybook {
    let freeze_prone_apps = apps
        .iter()
        .map(|a| FreezeProneAppInfo {
            id: a.id.clone(),
            label: a.label.clone(),
            category: a.category.clone(),
            remediation: a.remediation.clone(),
        })
        .collect();

    let critical = &profile.ui.critical_apps_summary;
    let erp_ip = &profile.erp_server_ip;

    MaintenancePlaybook {
        erp_server_ip: profile.erp_server_ip.clone(),
        erp_name: profile.erp_name.clone(),
        quick_start: ".\\executar-manutencao.ps1 -Perfil Rapido".into(),
        freeze_prone_apps,
        profiles: vec![
            MaintenanceProfile {
                id: "Rapido".into(),
                label: "Rapido (5-15 min)".into(),
                description: "Limpeza de temp, DNS, cache Windows, winget upgrade. Seguro em segundo plano.".into(),
                command: ".\\executar-manutencao.ps1 -Perfil Rapido".into(),
                estimated_minutes: 15,
            },
            MaintenanceProfile {
                id: "Completo".into(),
                label: "Completo (30-60 min)".into(),
                description: "Rapido + DISM, limpeza de componentes Windows, otimizacoes de disco.".into(),
                command: ".\\executar-manutencao.ps1 -Perfil Completo".into(),
                estimated_minutes: 45,
            },
            MaintenanceProfile {
                id: "AppsCriticos".into(),
                label: format!("Apps criticos ({critical})"),
                description: format!("Limpa cache dos programas que mais travam + teste do servidor {}.", profile.erp_name),
                command: ".\\executar-manutencao.ps1 -Perfil AppsCriticos".into(),
                estimated_minutes: 10,
            },
        ],
        tasks: vec![
            MaintenanceTask {
                id: "winget_upgrade".into(),
                label: "Atualizar tudo (winget)".into(),
                description: "Atualiza apps instalados via winget, incluindo LibreOffice e utilitarios.".into(),
                command: "winget upgrade --all --accept-package-agreements --accept-source-agreements --disable-interactivity".into(),
                profile: "Rapido".into(),
                requires_admin: false,
                runs_background: true,
            },
            MaintenanceTask {
                id: "temp_cleanup".into(),
                label: "Limpar arquivos temporarios".into(),
                description: "Remove %TEMP%, C:\\Windows\\Temp e cache de miniaturas.".into(),
                command: "Remove-Item $env:TEMP\\* -Recurse -Force -ErrorAction SilentlyContinue".into(),
                profile: "Rapido".into(),
                requires_admin: false,
                runs_background: true,
            },
            MaintenanceTask {
                id: "dns_flush".into(),
                label: "Limpar cache DNS".into(),
                description: format!("Resolve problemas de lentidao em apps que usam rede ({}).", profile.erp_name),
                command: "ipconfig /flushdns".into(),
                profile: "Rapido".into(),
                requires_admin: false,
                runs_background: false,
            },
            MaintenanceTask {
                id: "wu_check".into(),
                label: "Verificar Windows Update".into(),
                description: "Busca atualizacoes pendentes do Windows 10/11.".into(),
                command: "UsoClient StartScan".into(),
                profile: "Rapido".into(),
                requires_admin: true,
                runs_background: true,
            },
            MaintenanceTask {
                id: "dism_cleanup".into(),
                label: "DISM — limpar componentes".into(),
                description: "Libera espaco e corrige store do Windows (requer admin).".into(),
                command: "DISM /Online /Cleanup-Image /StartComponentCleanup".into(),
                profile: "Completo".into(),
                requires_admin: true,
                runs_background: true,
            },
            MaintenanceTask {
                id: "erp_ping".into(),
                label: format!("Testar servidor {} ERP", profile.erp_name),
                description: format!("Verifica se {erp_ip} responde na LAN."),
                command: format!("Test-Connection {erp_ip} -Count 3"),
                profile: "AppsCriticos".into(),
                requires_admin: false,
                runs_background: false,
            },
            MaintenanceTask {
                id: "fusion_cache".into(),
                label: "Limpar cache Fusion 360".into(),
                description: "Remove cache webdeploy da Autodesk.".into(),
                command: "Remove-Item \"$env:LOCALAPPDATA\\Autodesk\\webdeploy\\production\\*\\packages\" -Recurse -Force -ErrorAction SilentlyContinue".into(),
                profile: "AppsCriticos".into(),
                requires_admin: false,
                runs_background: true,
            },
            MaintenanceTask {
                id: "libre_cache".into(),
                label: "Limpar cache LibreOffice".into(),
                description: "Fecha travamentos do Draw/soffice.bin.".into(),
                command: "Remove-Item \"$env:APPDATA\\LibreOffice\\4\\cache\\*\" -Recurse -Force -ErrorAction SilentlyContinue".into(),
                profile: "AppsCriticos".into(),
                requires_admin: false,
                runs_background: true,
            },
        ],
    }
}
