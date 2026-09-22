//! Programas corporativos que costumam travar — planos de acao para o registro de incidentes.

use belarc_shared::CompanyProfile;

#[derive(Debug, Clone)]
pub struct FreezeProneApp {
    pub id: String,
    pub label: String,
    pub category: String,
    pub process_patterns: Vec<String>,
    pub product_patterns: Vec<String>,
    pub install_paths: Vec<String>,
    pub remediation: String,
}

pub fn build_freeze_prone_apps(profile: &CompanyProfile) -> Vec<FreezeProneApp> {
    let mut apps: Vec<FreezeProneApp> = profile
        .erp_branches
        .iter()
        .map(|branch| {
            let install_dir = branch
                .install_path
                .rsplit_once('\\')
                .map(|(dir, _)| dir)
                .unwrap_or(&branch.install_path);
            FreezeProneApp {
                id: branch.id.clone(),
                label: format!("{} — {}", branch.label, branch.install_path),
                category: "erp".into(),
                process_patterns: if branch.process_patterns.is_empty() {
                    vec!["sistema".into()]
                } else {
                    branch.process_patterns.clone()
                },
                product_patterns: if branch.product_patterns.is_empty() {
                    vec!["client.exe".into(), profile.erp_name.to_lowercase()]
                } else {
                    branch.product_patterns.clone()
                },
                install_paths: vec![branch.install_path.clone()],
                remediation: format!(
                    "1) Ping servidor ERP {} — se falhar, acionar TI rede. \
2) Reiniciar o cliente ERP (Gerenciador de Tarefas). \
3) Executar .\\executar-manutencao.ps1 -Perfil AppsCriticos. \
4) Verificar cabo de rede e switch da LAN. \
5) Se travar: limpar temp em {}\\temp.",
                    profile.erp_server_ip, install_dir
                ),
            }
        })
        .collect();

    apps.extend([
        FreezeProneApp {
            id: "fusion360".into(),
            label: "Fusion 360".into(),
            category: "engenharia".into(),
            process_patterns: vec!["fusion".into(), "fusion360".into(), "adsk".into()],
            product_patterns: vec![
                "fusion".into(),
                "autodesk fusion".into(),
                "fusion360".into(),
            ],
            install_paths: vec![],
            remediation: "1) Fechar Fusion e limpar cache: %LOCALAPPDATA%\\Autodesk\\webdeploy. \
2) Atualizar drivers de video (NVIDIA/AMD). \
3) Garantir 8+ GB RAM livre antes de abrir projetos grandes. \
4) winget upgrade Autodesk.Fusion360. \
5) Executar .\\executar-manutencao.ps1 -Perfil AppsCriticos."
                .into(),
        },
        FreezeProneApp {
            id: "autocad".into(),
            label: "AutoCAD".into(),
            category: "engenharia".into(),
            process_patterns: vec!["acad".into(), "autocad".into()],
            product_patterns: vec!["autocad".into(), "acad.exe".into()],
            install_paths: vec![],
            remediation: "1) Salvar DWG e reiniciar AutoCAD. \
2) Limpar %LOCALAPPDATA%\\Autodesk\\ADPSDK e temp AC*. \
3) PURGE e AUDIT no desenho se lento. \
4) Desativar hardware acceleration se travar ao renderizar. \
5) Executar .\\executar-manutencao.ps1 -Perfil AppsCriticos."
                .into(),
        },
        FreezeProneApp {
            id: "libreoffice".into(),
            label: "LibreOffice".into(),
            category: "escritorio".into(),
            process_patterns: vec!["soffice".into(), "libreoffice".into()],
            product_patterns: vec![
                "libreoffice".into(),
                "soffice.bin".into(),
                "soffice.exe".into(),
                "writer".into(),
                "calc".into(),
            ],
            install_paths: vec![],
            remediation: "1) Fechar todos os soffice.bin no Gerenciador de Tarefas. \
2) Limpar cache em %APPDATA%\\LibreOffice\\4\\cache. \
3) winget upgrade TheDocumentFoundation.LibreOffice. \
4) Executar .\\executar-manutencao.ps1 -Perfil AppsCriticos."
                .into(),
        },
        FreezeProneApp {
            id: "libredraw".into(),
            label: "LibreOffice Draw".into(),
            category: "escritorio".into(),
            process_patterns: vec!["soffice".into(), "libreoffice".into()],
            product_patterns: vec!["libreoffice".into(), "soffice.bin".into(), "draw".into()],
            install_paths: vec![],
            remediation: "1) Fechar todos os soffice.bin no Gerenciador de Tarefas. \
2) Limpar cache em %APPDATA%\\LibreOffice\\4\\cache. \
3) winget upgrade TheDocumentFoundation.LibreOffice. \
4) Abrir Draw com Recovery desabilitado se arquivo corrompido. \
5) Executar .\\executar-manutencao.ps1 -Perfil AppsCriticos."
                .into(),
        },
    ]);

    apps
}

pub fn match_product<'a>(product: &str, apps: &'a [FreezeProneApp]) -> Option<&'a FreezeProneApp> {
    let p = product.to_lowercase();
    apps.iter()
        .find(|app| app.product_patterns.iter().any(|pat| p.contains(pat)))
}

#[allow(dead_code)]
pub fn match_process<'a>(name: &str, apps: &'a [FreezeProneApp]) -> Option<&'a FreezeProneApp> {
    let n = name.to_lowercase();
    apps.iter()
        .find(|app| app.process_patterns.iter().any(|pat| n.contains(pat)))
}

pub fn erp_unreachable_remediation(profile: &CompanyProfile) -> String {
    format!(
        "Servidor ERP {} inalcancavel. Verificar: cabo de rede, switch, servidor ligado, \
ping {} no CMD. {} depende da LAN — sem servidor o cliente trava ou fica lento. \
Acionar TI de rede antes de reiniciar o PC.",
        profile.erp_server_ip, profile.erp_server_ip, profile.erp_name
    )
}
