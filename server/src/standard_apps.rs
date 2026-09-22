use belarc_shared::CollectorResult;
use belarc_shared::CompanyProfile;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StandardAppStatus {
    pub id: String,
    pub label: String,
    pub category: String,
    pub installed: bool,
    pub version: Option<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfficeKeyInfo {
    pub product: String,
    pub key_partial: Option<String>,
    pub license_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseKeysSummary {
    pub windows_key_partial: Option<String>,
    pub windows_product_key: Option<String>,
    pub office_keys: Vec<OfficeKeyInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsetInfo {
    pub installed: bool,
    pub product_name: Option<String>,
    pub version: Option<String>,
    pub agent_version: Option<String>,
    pub install_path: Option<String>,
    pub service_running: bool,
    pub real_time_active: Option<bool>,
}

pub fn detect_standard_apps(
    collectors: &[CollectorResult],
    profile: &CompanyProfile,
) -> Vec<StandardAppStatus> {
    let mut apps =
        load_from_software_collector(collectors).unwrap_or_else(|| legacy_detect(collectors));
    enrich_anydesk(&mut apps, collectors);
    enrich_thunderbird(&mut apps, collectors);
    enrich_eset_app(&mut apps, collectors, profile);
    enrich_affinity(&mut apps, collectors);
    ensure_required_apps(&mut apps, profile);
    sort_standard_apps(apps, profile)
}

fn enrich_affinity(apps: &mut Vec<StandardAppStatus>, collectors: &[CollectorResult]) {
    let Some(software) = collectors.iter().find(|c| c.name == "software") else {
        return;
    };
    if let Some(arr) = software
        .data
        .get("standard_apps")
        .and_then(|v| v.as_array())
    {
        for item in arr {
            if item.get("id").and_then(|v| v.as_str()) != Some("affinity") {
                continue;
            }
            upsert_app(
                apps,
                StandardAppStatus {
                    id: "affinity".into(),
                    label: item
                        .get("label")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Affinity Canva")
                        .into(),
                    category: "design".into(),
                    installed: item
                        .get("installed")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    version: item
                        .get("version")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    detail: item
                        .get("detail")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                },
            );
            return;
        }
    }
}

fn ensure_required_apps(apps: &mut Vec<StandardAppStatus>, profile: &CompanyProfile) {
    let mut required: Vec<(&str, &str, &str)> = vec![
        (
            profile.banking_app.id.as_str(),
            profile.banking_app.label.as_str(),
            "bancario",
        ),
        ("office", "Microsoft Office", "escritorio"),
        ("excel", "Microsoft Excel", "escritorio"),
        ("word", "Microsoft Word", "escritorio"),
        ("eset", profile.antivirus_name.as_str(), "seguranca"),
        ("anydesk", "AnyDesk", "acesso"),
        ("thunderbird", "Thunderbird", "email"),
        ("affinity", "Affinity Canva", "design"),
    ];
    for branch in &profile.erp_branches {
        required.push((branch.id.as_str(), branch.label.as_str(), "erp"));
    }

    for (id, label, category) in required {
        if !apps.iter().any(|a| a.id == id) {
            apps.push(StandardAppStatus {
                id: id.into(),
                label: label.into(),
                category: category.into(),
                installed: false,
                version: None,
                detail: None,
            });
        }
    }
}

fn load_from_software_collector(collectors: &[CollectorResult]) -> Option<Vec<StandardAppStatus>> {
    let software = collectors.iter().find(|c| c.name == "software")?;
    let arr = software.data.get("standard_apps")?.as_array()?;
    if arr.is_empty() {
        return None;
    }
    Some(
        arr.iter()
            .filter_map(|v| {
                Some(StandardAppStatus {
                    id: v.get("id")?.as_str()?.to_string(),
                    label: v.get("label")?.as_str()?.to_string(),
                    category: v.get("category")?.as_str()?.to_string(),
                    installed: v
                        .get("installed")
                        .and_then(|x| x.as_bool())
                        .unwrap_or(false),
                    version: v.get("version").and_then(|x| x.as_str()).map(String::from),
                    detail: v.get("detail").and_then(|x| x.as_str()).map(String::from),
                })
            })
            .collect(),
    )
}

fn enrich_anydesk(apps: &mut Vec<StandardAppStatus>, collectors: &[CollectorResult]) {
    let Some(ra) = collectors.iter().find(|c| c.name == "remote_access") else {
        return;
    };
    let Some(ad) = ra.data.get("anydesk") else {
        return;
    };
    let installed = ad
        .get("installed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    upsert_app(
        apps,
        StandardAppStatus {
            id: "anydesk".into(),
            label: "AnyDesk".into(),
            category: "acesso".into(),
            installed,
            version: ad.get("version").and_then(|v| v.as_str()).map(String::from),
            detail: ad
                .get("client_id")
                .and_then(|v| v.as_str())
                .map(|id| format!("ID {id}")),
        },
    );
}

fn enrich_thunderbird(apps: &mut Vec<StandardAppStatus>, collectors: &[CollectorResult]) {
    let Some(em) = collectors.iter().find(|c| c.name == "email") else {
        return;
    };
    let Some(tb) = em.data.get("thunderbird_summary") else {
        return;
    };
    let installed = tb
        .get("installed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let count = tb
        .get("profile_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    upsert_app(
        apps,
        StandardAppStatus {
            id: "thunderbird".into(),
            label: "Thunderbird".into(),
            category: "email".into(),
            installed,
            version: None,
            detail: if count > 0 {
                Some(format!("{count} perfil(is)"))
            } else {
                None
            },
        },
    );
}

fn enrich_eset_app(
    apps: &mut Vec<StandardAppStatus>,
    collectors: &[CollectorResult],
    profile: &CompanyProfile,
) {
    let info = extract_eset_info(collectors);
    if !info.installed {
        return;
    }
    let mut detail_parts = vec![];
    if let Some(ref n) = info.product_name {
        detail_parts.push(n.clone());
    }
    if let Some(ref v) = info.version {
        detail_parts.push(format!("v{v}"));
    }
    if info.service_running {
        detail_parts.push("Serviço ativo".into());
    }
    if let Some(ref p) = info.install_path {
        detail_parts.push(p.clone());
    }
    upsert_app(
        apps,
        StandardAppStatus {
            id: "eset".into(),
            label: profile.antivirus_name.clone(),
            category: "seguranca".into(),
            installed: true,
            version: info.version.clone(),
            detail: if detail_parts.is_empty() {
                None
            } else {
                Some(detail_parts.join(" · "))
            },
        },
    );
}

fn upsert_app(apps: &mut Vec<StandardAppStatus>, app: StandardAppStatus) {
    if let Some(existing) = apps.iter_mut().find(|a| a.id == app.id) {
        *existing = app;
    } else {
        apps.push(app);
    }
}

fn sort_standard_apps(
    mut apps: Vec<StandardAppStatus>,
    profile: &CompanyProfile,
) -> Vec<StandardAppStatus> {
    let mut order: Vec<&str> = vec!["office", "excel", "word"];
    for branch in &profile.erp_branches {
        order.push(branch.id.as_str());
    }
    order.extend([
        "eset",
        "anydesk",
        "thunderbird",
        "autocad",
        "fusion",
        "libredraw",
        "lightshot",
        "pdf24",
        "sumatra",
        "gimp",
        "affinity",
        "vscode",
        profile.banking_app.id.as_str(),
    ]);
    apps.sort_by(|a, b| {
        let ia = order.iter().position(|x| *x == a.id).unwrap_or(999);
        let ib = order.iter().position(|x| *x == b.id).unwrap_or(999);
        ia.cmp(&ib).then(a.label.cmp(&b.label))
    });
    apps
}

fn legacy_detect(collectors: &[CollectorResult]) -> Vec<StandardAppStatus> {
    let programs = collectors
        .iter()
        .find(|c| c.name == "software")
        .and_then(|c| c.data.get("programs"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let defs: &[(&str, &str, &str, &[&str])] = &[
        ("autocad", "AutoCAD", "engenharia", &["autocad"]),
        ("fusion", "Fusion 360", "engenharia", &["fusion"]),
        (
            "excel",
            "Microsoft Excel",
            "escritorio",
            &["microsoft excel", "excel"],
        ),
        ("word", "Microsoft Word", "escritorio", &["microsoft word"]),
        ("eset", "ESET", "seguranca", &["eset"]),
        (
            "affinity",
            "Affinity Canva",
            "design",
            &["affinity canva", "affinity", "canva.affinity"],
        ),
        (
            "vscode",
            "VS Code",
            "dev",
            &["visual studio code", "vscode"],
        ),
    ];

    defs.iter()
        .map(|(id, label, cat, pats)| {
            let hit = programs.iter().find(|p| {
                let name = p
                    .get("display_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_lowercase();
                pats.iter().any(|pat| name.contains(pat))
            });
            StandardAppStatus {
                id: (*id).into(),
                label: (*label).into(),
                category: (*cat).into(),
                installed: hit.is_some(),
                version: hit
                    .and_then(|p| p.get("version"))
                    .and_then(|v| v.as_str())
                    .map(String::from),
                detail: hit
                    .and_then(|p| p.get("display_name"))
                    .and_then(|v| v.as_str())
                    .map(String::from),
            }
        })
        .collect()
}

pub fn extract_eset_info(collectors: &[CollectorResult]) -> EsetInfo {
    if let Some(sec) = collectors.iter().find(|c| c.name == "security") {
        if let Some(eset) = sec.data.get("eset") {
            return EsetInfo {
                installed: eset
                    .get("installed")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                product_name: eset
                    .get("product_name")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                version: eset
                    .get("version")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                agent_version: eset
                    .get("agent_version")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                install_path: eset
                    .get("install_path")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                service_running: eset
                    .get("service_running")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                real_time_active: eset.get("real_time_active").and_then(|v| v.as_bool()),
            };
        }
    }
    EsetInfo {
        installed: false,
        product_name: None,
        version: None,
        agent_version: None,
        install_path: None,
        service_running: false,
        real_time_active: None,
    }
}

pub fn extract_license_keys(collectors: &[CollectorResult]) -> LicenseKeysSummary {
    let mut summary = LicenseKeysSummary {
        windows_key_partial: None,
        windows_product_key: None,
        office_keys: vec![],
    };

    if let Some(lic) = collectors.iter().find(|c| c.name == "licensing") {
        if let Some(win) = lic.data.get("windows") {
            summary.windows_key_partial = win
                .get("product_key_partial")
                .and_then(|v| v.as_str())
                .map(String::from);
            summary.windows_product_key = win
                .get("product_key")
                .and_then(|v| v.as_str())
                .map(String::from);
        }
        if let Some(office) = lic.data.get("office").and_then(|v| v.as_array()) {
            for o in office {
                let name = o
                    .get("license_name")
                    .and_then(|v| v.as_str())
                    .or_else(|| o.get("product_id").and_then(|v| v.as_str()))
                    .unwrap_or("Office");
                let key = o
                    .get("key_partial")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let status = o
                    .get("license_status")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                summary.office_keys.push(OfficeKeyInfo {
                    product: name.to_string(),
                    key_partial: key,
                    license_status: status,
                });
            }
        }
    }

    summary
}

pub fn eset_status(collectors: &[CollectorResult]) -> (bool, Option<String>) {
    let info = extract_eset_info(collectors);
    if !info.installed {
        return (false, None);
    }
    let label = info
        .product_name
        .or(info.version.map(|v| format!("ESET v{v}")))
        .or_else(|| Some("ESET instalado".into()));
    (true, label)
}
