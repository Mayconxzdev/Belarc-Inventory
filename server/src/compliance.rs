use std::collections::{HashMap, HashSet};

use belarc_shared::{
    alert_stable_id, AlertRecord, AlertSeverity, CollectorResult, MachineSummary, ServerConfig,
};

use crate::admin::MachineAdmin;
use crate::standard_apps;

/// Pesos do score corporativo (soma = 100).
pub const WEIGHT_SMART_DISK: u8 = 30;
pub const WEIGHT_SECURITY: u8 = 25;
pub const WEIGHT_WINDOWS_UPDATE: u8 = 15;
pub const WEIGHT_CRITICAL_EVENTS: u8 = 10;
pub const WEIGHT_DISK_FREE: u8 = 10;
pub const WEIGHT_TEMPERATURE: u8 = 5;
pub const WEIGHT_CERTIFICATES: u8 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertClass {
    Corporate,
    RootHistorical,
    SelfSigned,
    Other,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ScoreBreakdown {
    pub total: u8,
    pub band: String,
    pub smart_disk: u8,
    pub security: u8,
    pub windows_update: u8,
    pub critical_events: u8,
    pub disk_free: u8,
    pub temperature: u8,
    pub certificates: u8,
    pub notes: Vec<String>,
}

pub fn score_band(total: u8) -> &'static str {
    match total {
        90..=100 => "Excelente",
        75..=89 => "Bom",
        50..=74 => "Atenção",
        25..=49 => "Problema",
        _ => "Crítico",
    }
}

impl ScoreBreakdown {
    pub fn to_markdown(&self) -> String {
        let mut md = String::from("## Conformidade TI\n\n");
        md.push_str(&format!("**Score: {}% — {}**\n\n", self.total, self.band));
        md.push_str("| Categoria | Pontos | Máximo |\n");
        md.push_str("|-----------|-------:|-------:|\n");
        md.push_str(&format!(
            "| Disco SMART | {} | {} |\n",
            self.smart_disk, WEIGHT_SMART_DISK
        ));
        md.push_str(&format!(
            "| Segurança (AV + firewall) | {} | {} |\n",
            self.security, WEIGHT_SECURITY
        ));
        md.push_str(&format!(
            "| Windows Update | {} | {} |\n",
            self.windows_update, WEIGHT_WINDOWS_UPDATE
        ));
        md.push_str(&format!(
            "| Eventos críticos | {} | {} |\n",
            self.critical_events, WEIGHT_CRITICAL_EVENTS
        ));
        md.push_str(&format!(
            "| Espaço em disco | {} | {} |\n",
            self.disk_free, WEIGHT_DISK_FREE
        ));
        md.push_str(&format!(
            "| Temperaturas | {} | {} |\n",
            self.temperature, WEIGHT_TEMPERATURE
        ));
        md.push_str(&format!(
            "| Certificados corporativos | {} | {} |\n",
            self.certificates, WEIGHT_CERTIFICATES
        ));
        if !self.notes.is_empty() {
            md.push_str("\n**Observações:**\n");
            for n in &self.notes {
                md.push_str(&format!("- {n}\n"));
            }
        }
        md.push('\n');
        md
    }
}

struct AlertBuilder {
    machine_id: String,
    hostname: String,
    seen: HashSet<String>,
    alerts: Vec<AlertRecord>,
}

impl AlertBuilder {
    fn new(machine: &MachineSummary) -> Self {
        Self {
            machine_id: machine.id.clone(),
            hostname: machine.hostname.clone(),
            seen: HashSet::new(),
            alerts: Vec::new(),
        }
    }

    fn add(
        &mut self,
        severity: AlertSeverity,
        category: &str,
        key: &str,
        message: impl Into<String>,
    ) {
        let dedupe = format!("{category}:{key}");
        if !self.seen.insert(dedupe) {
            return;
        }
        self.alerts.push(AlertRecord {
            id: alert_stable_id(&self.machine_id, category, key),
            machine_id: self.machine_id.clone(),
            hostname: self.hostname.clone(),
            severity,
            category: category.into(),
            message: message.into(),
            created_at: chrono::Utc::now(),
            resolved: false,
        });
    }

    fn finish(self) -> Vec<AlertRecord> {
        self.alerts
    }
}

fn cert_short_name(subject: &str) -> String {
    let s = subject.trim();
    if let Some(rest) = s.strip_prefix("CN=") {
        return rest.split(',').next().unwrap_or(s).trim().to_string();
    }
    if s.len() > 80 {
        format!("{}…", &s[..77])
    } else {
        s.to_string()
    }
}

fn json_f64(v: &serde_json::Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_u64().map(|n| n as f64))
}

fn ram_total_gb(hw: &CollectorResult) -> Option<f64> {
    hw.data
        .get("ram")
        .and_then(|r| r.get("total_gb"))
        .and_then(json_f64)
        .filter(|v| *v > 0.0)
        .or_else(|| {
            hw.data
                .get("system")
                .and_then(|s| s.get("total_physical_memory_gb"))
                .and_then(json_f64)
                .filter(|v| *v > 0.0)
        })
}

fn json_u64(v: &serde_json::Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_i64().map(|n| n.max(0) as u64))
}

fn json_bool(v: &serde_json::Value) -> Option<bool> {
    v.as_bool()
}

fn is_known_root_issuer(subject: &str, issuer: &str) -> bool {
    let s = format!("{subject} {issuer}").to_uppercase();
    [
        "MICROSOFT ROOT",
        "BALTIMORE",
        "CYBERTRUST",
        "VERISIGN",
        "THAWTE",
        "COMODO",
        "DIGICERT",
        "GLOBALSIGN",
        "ENTRUST",
        "GEOTRUST",
        "STARFIELD",
        "USERTRUST",
        "AAA CERTIFICATE SERVICES",
        "IDENTITY VERIFICATION ROOT",
    ]
    .iter()
    .any(|k| s.contains(k))
}

pub fn classify_certificate(cert: &serde_json::Value) -> CertClass {
    let store = cert.get("store").and_then(|s| s.as_str()).unwrap_or("");
    let subject = cert.get("subject").and_then(|s| s.as_str()).unwrap_or("");
    let issuer = cert.get("issuer").and_then(|s| s.as_str()).unwrap_or("");
    let has_pk = cert
        .get("has_private_key")
        .and_then(json_bool)
        .unwrap_or(false);
    let subject_uc = subject.to_uppercase();

    if store.contains("\\Root") && !has_pk {
        if is_known_root_issuer(subject, issuer) || issuer == subject {
            return CertClass::RootHistorical;
        }
    }

    let cn = subject_uc
        .strip_prefix("CN=")
        .and_then(|c| c.split(',').next())
        .unwrap_or(&subject_uc);
    if has_pk && (cn.contains("PROJETO") || cn.contains("DESKTOP") || cn.contains("WILLIAM"))
        || (issuer == subject && has_pk)
    {
        return CertClass::SelfSigned;
    }

    if store.contains("\\My") && has_pk {
        if subject_uc.contains("ICP-BRASIL")
            || subject_uc.contains("E-CNPJ")
            || subject_uc.contains("E-CPF")
            || subject_uc.contains("RECEITA FEDERAL")
            || subject_uc.contains("RFB")
        {
            return CertClass::Corporate;
        }
        if let Some(name) = subject_uc.strip_prefix("CN=") {
            let cn_part = name.split(',').next().unwrap_or(name);
            if cn_part.contains(':') {
                let parts: Vec<_> = cn_part.split(':').collect();
                if parts.len() >= 2
                    && parts[1].chars().all(|c| c.is_ascii_digit())
                    && parts[1].len() >= 11
                {
                    return CertClass::Corporate;
                }
            }
        }
    }

    if store.contains("\\My") {
        return CertClass::Other;
    }

    CertClass::RootHistorical
}

fn corporate_cert_entity_key(subject: &str) -> String {
    let subject_uc = subject.to_uppercase();
    let cn_part = subject_uc
        .strip_prefix("CN=")
        .and_then(|c| c.split(',').next())
        .unwrap_or(&subject_uc);
    if let Some(cnpj) = cn_part.split(':').nth(1) {
        let digits: String = cnpj.chars().filter(|c| c.is_ascii_digit()).collect();
        if digits.len() >= 11 {
            return digits;
        }
    }
    cn_part
        .split(':')
        .next()
        .unwrap_or(cn_part)
        .trim()
        .to_string()
}

/// Melhor validade (days_left) por entidade corporativa (CNPJ ou razão social).
fn corporate_entities_best_days(certs: &CollectorResult) -> HashMap<String, (i64, String)> {
    let mut by_entity: HashMap<String, (i64, String)> = HashMap::new();
    for key in ["certificates", "expiring_soon", "expired"] {
        let Some(arr) = certs.data.get(key).and_then(|v| v.as_array()) else {
            continue;
        };
        for cert in arr {
            if classify_certificate(cert) != CertClass::Corporate {
                continue;
            }
            let subject = cert.get("subject").and_then(|s| s.as_str()).unwrap_or("?");
            let entity = corporate_cert_entity_key(subject);
            let short = cert_short_name(subject);
            let days = cert
                .get("days_left")
                .and_then(|d| d.as_i64())
                .unwrap_or(if key == "expired" { -1 } else { 30 });
            by_entity
                .entry(entity)
                .and_modify(|(best, name)| {
                    if days > *best {
                        *best = days;
                        *name = short.clone();
                    }
                })
                .or_insert((days, short));
        }
    }
    by_entity
}

fn score_smart_disk(hw: Option<&CollectorResult>, notes: &mut Vec<String>) -> u8 {
    let Some(hw) = hw else {
        notes.push("SMART: dados indisponíveis — neutro.".into());
        return 22;
    };
    let physical = match hw.data.get("physical_disks").and_then(|d| d.as_array()) {
        Some(p) if !p.is_empty() => p,
        _ => {
            notes.push("SMART: sem discos físicos — neutro.".into());
            return 22;
        }
    };

    let mut any_unhealthy = false;
    let mut any_warning = false;

    for pd in physical {
        let health = pd
            .get("health_status")
            .and_then(|h| h.as_str())
            .unwrap_or("");
        if health.eq_ignore_ascii_case("Unhealthy") {
            any_unhealthy = true;
        } else if health.eq_ignore_ascii_case("Warning") {
            any_warning = true;
        }
    }

    if any_unhealthy {
        notes.push("SMART: falha iminente detectada.".into());
        return 0;
    }
    if any_warning {
        notes.push("SMART: alerta de saúde no disco.".into());
        return WEIGHT_SMART_DISK.saturating_mul(40).saturating_div(100);
    }
    notes.push("SMART: discos saudáveis.".into());
    WEIGHT_SMART_DISK
}

fn score_security(security: Option<&CollectorResult>, notes: &mut Vec<String>) -> u8 {
    let Some(sec) = security else {
        notes.push("Segurança: dados indisponíveis.".into());
        return 0;
    };

    let mut score = WEIGHT_SECURITY;

    let eset = sec.data.get("eset");
    let eset_installed = eset
        .and_then(|e| e.get("installed"))
        .and_then(json_bool)
        .unwrap_or(false);
    let eset_running = eset
        .and_then(|e| e.get("service_running"))
        .and_then(json_bool)
        .unwrap_or(false);

    let mut av_ok = false;
    if eset_installed && eset_running {
        av_ok = true;
    } else if let Some(defender) = sec.data.get("defender") {
        let enabled = defender.get("enabled").and_then(json_bool).unwrap_or(false);
        if enabled {
            av_ok = true;
            if !eset_installed {
                notes.push("Segurança: Defender ativo (ESET ausente).".into());
                score = score.saturating_sub(5);
            }
        }
    }

    if !av_ok {
        if let Some(products) = sec.data.get("av_products").and_then(|p| p.as_array()) {
            av_ok = products
                .iter()
                .any(|p| p.get("enabled").and_then(json_bool).unwrap_or(false));
            if av_ok {
                notes.push("Segurança: antivírus de terceiros ativo (WSC).".into());
            }
        }
    }

    if !av_ok {
        notes.push("Segurança: antivírus desativado.".into());
        score = score.saturating_sub(20);
    } else if eset_installed && !eset_running {
        notes.push("Segurança: ESET instalado mas serviço parado.".into());
        score = score.saturating_sub(12);
    }

    if let Some(firewall) = sec.data.get("firewall") {
        let enabled = firewall.get("enabled").and_then(json_bool).unwrap_or(true);
        if !enabled {
            if eset_installed && eset_running {
                notes.push("Segurança: firewall Windows off (ESET ativo).".into());
                score = score.saturating_sub(2);
            } else {
                notes.push("Segurança: firewall desativado.".into());
                score = score.saturating_sub(8);
            }
        }
    }

    score
}

fn score_windows_update(
    os_col: Option<&CollectorResult>,
    compliance: Option<&CollectorResult>,
    notes: &mut Vec<String>,
) -> u8 {
    let mut score = WEIGHT_WINDOWS_UPDATE;
    let pending = compliance
        .and_then(|c| c.data.get("pending_updates"))
        .and_then(json_u64)
        .unwrap_or(0);

    if let Some(os) = os_col {
        if let Some(build) = os.data.get("build").and_then(|v| v.as_str()) {
            if let Ok(build_n) = build.parse::<u32>() {
                if build_n < 19041 {
                    notes.push(format!("Windows Update: build antiga ({build})."));
                    score = score.saturating_sub(10);
                }
            }
        }
    }

    if pending > 20 {
        notes.push(format!("Windows Update: {pending} pendentes."));
        score = score.saturating_sub(12);
    } else if pending > 5 {
        score = score.saturating_sub(6);
    } else if pending > 0 {
        score = score.saturating_sub(3);
    }

    score
}

fn score_disk_free(hw: Option<&CollectorResult>, notes: &mut Vec<String>) -> u8 {
    let Some(hw) = hw else {
        return 7;
    };
    let disks = hw
        .data
        .get("logical_disks")
        .or_else(|| hw.data.get("disks"))
        .and_then(|d| d.as_array());

    let Some(disks) = disks else {
        return 7;
    };

    let mut worst_free = 100.0f64;
    for disk in disks {
        let free_pct = disk.get("free_percent").and_then(json_f64).unwrap_or(100.0);
        worst_free = worst_free.min(free_pct);
    }

    if worst_free < 5.0 {
        notes.push(format!("Disco: {worst_free:.1}% livre — crítico."));
        return 0;
    }
    if worst_free < 10.0 {
        return WEIGHT_DISK_FREE.saturating_mul(30).saturating_div(100);
    }
    if worst_free < 15.0 {
        return WEIGHT_DISK_FREE.saturating_mul(50).saturating_div(100);
    }
    if worst_free < 25.0 {
        return WEIGHT_DISK_FREE.saturating_mul(75).saturating_div(100);
    }
    WEIGHT_DISK_FREE
}

fn score_critical_events(event_logs: Option<&CollectorResult>, notes: &mut Vec<String>) -> u8 {
    let mut score = WEIGHT_CRITICAL_EVENTS;

    if let Some(ev) = event_logs {
        if let Some(summary) = ev.data.get("summary") {
            let bsod_count = summary.get("bsod_count").and_then(json_u64).unwrap_or(0);
            let unexpected = summary
                .get("unexpected_shutdowns")
                .and_then(json_u64)
                .unwrap_or(0);
            let sys_err = summary
                .get("system_errors_count")
                .and_then(json_u64)
                .unwrap_or(0);

            if bsod_count > 0 {
                notes.push("Eventos: BSOD recente.".into());
                score = 0;
            } else if unexpected > 0 {
                notes.push(format!(
                    "Eventos: {unexpected} desligamento(s) inesperado(s)."
                ));
                score = score.saturating_sub(3);
            }

            if sys_err > 50 {
                notes.push(format!("Eventos: {sys_err} erros System (14d)."));
                score = score.saturating_sub(4);
            } else if sys_err > 20 {
                score = score.saturating_sub(2);
            }
        }
    }

    score
}

fn score_temperature(hw: Option<&CollectorResult>, notes: &mut Vec<String>) -> u8 {
    if let Some(hw) = hw {
        if let Some(temps) = hw.data.get("temperatures").and_then(|t| t.as_array()) {
            if !temps.is_empty() {
                let mut worst = 0.0f64;
                for t in temps {
                    if let Some(v) = t.get("celsius").and_then(json_f64) {
                        worst = worst.max(v);
                    }
                }
                if worst > 90.0 {
                    notes.push(format!("Temperatura: {worst:.0}°C — crítico."));
                    return 0;
                }
                if worst > 80.0 {
                    notes.push(format!("Temperatura: {worst:.0}°C — alta."));
                    return 2;
                }
                return WEIGHT_TEMPERATURE;
            }
        }
    }
    notes.push("Temperatura: coletor indisponível — neutro.".into());
    WEIGHT_TEMPERATURE.saturating_mul(90).saturating_div(100)
}

fn score_certificates(certs: Option<&CollectorResult>, notes: &mut Vec<String>) -> u8 {
    let Some(c) = certs else {
        notes.push("Certificados: dados indisponíveis — neutro.".into());
        return WEIGHT_CERTIFICATES.saturating_mul(80).saturating_div(100);
    };

    let entities = corporate_entities_best_days(c);
    if entities.is_empty() {
        return WEIGHT_CERTIFICATES;
    }

    let worst = entities.values().map(|(d, _)| *d).min().unwrap_or(365);
    let expired: Vec<_> = entities
        .iter()
        .filter(|(_, (d, _))| *d < 0)
        .map(|(_, (_, n))| n.as_str())
        .collect();
    let expiring: Vec<_> = entities
        .iter()
        .filter(|(_, (d, _))| (0..=30).contains(d))
        .map(|(_, (_, n))| n.as_str())
        .collect();

    if worst >= 31 {
        return WEIGHT_CERTIFICATES;
    }
    if worst >= 0 {
        notes.push(format!(
            "Certificado corporativo expirando em breve: {}.",
            expiring.join(", ")
        ));
        return WEIGHT_CERTIFICATES.saturating_mul(60).saturating_div(100);
    }
    if expired.len() == 1 {
        notes.push(format!(
            "Certificado corporativo expirado: {} — renovar ou remover do repositório.",
            expired[0]
        ));
        return WEIGHT_CERTIFICATES.saturating_mul(40).saturating_div(100);
    }
    notes.push(format!(
        "Certificados corporativos expirados: {}.",
        expired.join(", ")
    ));
    0
}

pub fn compute_score_breakdown(
    _machine: &MachineSummary,
    collectors: &[CollectorResult],
) -> ScoreBreakdown {
    let hardware = collectors.iter().find(|c| c.name == "hardware");
    let security = collectors.iter().find(|c| c.name == "security");
    let certificates = collectors.iter().find(|c| c.name == "certificates");
    let event_logs = collectors.iter().find(|c| c.name == "event_logs");
    let os_col = collectors.iter().find(|c| c.name == "os");
    let compliance_col = collectors.iter().find(|c| c.name == "compliance");

    let mut notes = Vec::new();

    let smart_disk = score_smart_disk(hardware, &mut notes);
    let security_score = score_security(security, &mut notes);
    let windows_update = score_windows_update(os_col, compliance_col, &mut notes);
    let critical_events = score_critical_events(event_logs, &mut notes);
    let disk_free = score_disk_free(hardware, &mut notes);
    let temperature = score_temperature(hardware, &mut notes);
    let certificates_score = score_certificates(certificates, &mut notes);

    let total = smart_disk
        .saturating_add(security_score)
        .saturating_add(windows_update)
        .saturating_add(critical_events)
        .saturating_add(disk_free)
        .saturating_add(temperature)
        .saturating_add(certificates_score)
        .min(100);

    let band = score_band(total).to_string();

    ScoreBreakdown {
        total,
        band,
        smart_disk,
        security: security_score,
        windows_update,
        critical_events,
        disk_free,
        temperature,
        certificates: certificates_score,
        notes,
    }
}

fn build_alerts(
    machine: &MachineSummary,
    collectors: &[CollectorResult],
    config: &ServerConfig,
    admin: Option<&MachineAdmin>,
    profile: &belarc_shared::CompanyProfile,
) -> Vec<AlertRecord> {
    let mut b = AlertBuilder::new(machine);

    let software = collectors.iter().find(|c| c.name == "software");
    let security = collectors.iter().find(|c| c.name == "security");
    let certificates = collectors.iter().find(|c| c.name == "certificates");
    let hardware = collectors.iter().find(|c| c.name == "hardware");
    let event_logs = collectors.iter().find(|c| c.name == "event_logs");
    let os_col = collectors.iter().find(|c| c.name == "os");

    let eset_installed = security
        .and_then(|s| s.data.get("eset"))
        .and_then(|e| e.get("installed"))
        .and_then(json_bool)
        .unwrap_or(false);

    // Software policy alerts
    if let Some(sw) = software {
        if let Some(programs) = sw.data.get("programs").and_then(|p| p.as_array()) {
            for blocked in &config.blacklist_software {
                let blocked_lc = blocked.to_lowercase();
                if programs.iter().any(|prog| {
                    prog.get("display_name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&blocked_lc)
                }) {
                    b.add(
                        AlertSeverity::Warning,
                        "blacklist",
                        blocked,
                        format!("Software não autorizado: {blocked}"),
                    );
                }
            }
        }

        if let Some(startup) = sw.data.get("startup").and_then(|s| s.as_array()) {
            let count = startup.len();
            if count > 15 {
                b.add(
                    AlertSeverity::Warning,
                    "performance",
                    "startup_high",
                    format!("{count} programas na inicialização — revisar"),
                );
            }
        }
    }

    // Standard apps missing
    let apps = standard_apps::detect_standard_apps(collectors, profile);
    for app in &apps {
        if !app.installed {
            b.add(
                AlertSeverity::Warning,
                "software_policy",
                &app.id,
                format!("Programa padrão ausente: {}", app.label),
            );
        }
    }

    // Security alerts
    if let Some(sec) = security {
        if !eset_installed {
            b.add(
                AlertSeverity::Critical,
                "antivirus",
                "eset_missing",
                "ESET (antivírus corporativo) não detectado",
            );
        } else if sec
            .data
            .get("eset")
            .and_then(|e| e.get("service_running"))
            .and_then(json_bool)
            == Some(false)
        {
            b.add(
                AlertSeverity::Critical,
                "antivirus",
                "eset_stopped",
                "Serviço ESET parado",
            );
        }

        if let Some(defender) = sec.data.get("defender") {
            let enabled = defender.get("enabled").and_then(json_bool).unwrap_or(false);
            if !eset_installed && !enabled {
                b.add(
                    AlertSeverity::Critical,
                    "antivirus",
                    "defender_off",
                    "Windows Defender desativado e sem ESET",
                );
            }
        }

        if let Some(firewall) = sec.data.get("firewall") {
            if firewall.get("enabled").and_then(json_bool) == Some(false) {
                b.add(
                    AlertSeverity::Warning,
                    "firewall",
                    "firewall_off",
                    "Firewall desativado",
                );
            }
        }

        // BitLocker — alerta informativo apenas (não afeta score)
        if let Some(bl) = sec.data.get("bitlocker").and_then(|v| v.as_array()) {
            let system_vol = bl.iter().find(|v| {
                v.get("mount_point")
                    .and_then(|m| m.as_str())
                    .map(|m| m.starts_with("C:"))
                    .unwrap_or(false)
            });
            if let Some(vol) = system_vol {
                let prot = vol
                    .get("protection_status")
                    .and_then(|s| s.as_str())
                    .unwrap_or("");
                if prot != "On" && prot != "1" {
                    b.add(
                        AlertSeverity::Info,
                        "bitlocker",
                        "c_drive",
                        "BitLocker não ativo no volume C: — informativo",
                    );
                }
            }
        }
    }

    // Certificates — um alerta por entidade corporativa (pior status relevante)
    if let Some(certs) = certificates {
        for (_, (days, short)) in corporate_entities_best_days(certs) {
            if days >= 31 {
                continue;
            }
            let key = format!("corp_{short}");
            let msg = if days < 0 {
                format!("Certificado corporativo expirado: {short}")
            } else {
                format!("Certificado corporativo expira em breve: {short} ({days}d)")
            };
            b.add(AlertSeverity::Warning, "certificate", &key, msg);
        }
    }

    // Disk / SMART / RAM (alert only for RAM)
    if let Some(hw) = hardware {
        if let Some(disks) = hw
            .data
            .get("logical_disks")
            .or_else(|| hw.data.get("disks"))
            .and_then(|d| d.as_array())
        {
            for disk in disks {
                let free_pct = disk.get("free_percent").and_then(json_f64).unwrap_or(100.0);
                let letter = disk.get("letter").and_then(|l| l.as_str()).unwrap_or("?");
                if free_pct < 15.0 {
                    b.add(
                        AlertSeverity::Warning,
                        "disk",
                        &format!("disk_{letter}"),
                        format!("Disco {letter} com {free_pct:.1}% livre"),
                    );
                }
            }
        }

        if let Some(physical) = hw.data.get("physical_disks").and_then(|d| d.as_array()) {
            for (i, pd) in physical.iter().enumerate() {
                let health = pd
                    .get("health_status")
                    .and_then(|h| h.as_str())
                    .unwrap_or("");
                if health.eq_ignore_ascii_case("Unhealthy") {
                    b.add(
                        AlertSeverity::Critical,
                        "disk",
                        &format!("health_{i}"),
                        "SMART: disco com falha iminente",
                    );
                } else if health.eq_ignore_ascii_case("Warning") {
                    b.add(
                        AlertSeverity::Warning,
                        "disk",
                        &format!("health_warn_{i}"),
                        "SMART: disco com alerta de saúde",
                    );
                }
            }
        }

        if let Some(ram) = ram_total_gb(hw) {
            if ram < 8.0 {
                b.add(
                    AlertSeverity::Warning,
                    "performance",
                    "ram_low",
                    format!("RAM limitada ({ram:.1} GB) — 16 GB recomendado"),
                );
            }
        }
    }

    // Uptime alert (>15 days)
    if let Some(uptime) = machine.uptime_seconds {
        let days = uptime / 86400;
        if days > 15 {
            b.add(
                AlertSeverity::Warning,
                "performance",
                "uptime_long",
                format!("Uptime de {days} dias — agendar reinício"),
            );
        }
    }

    // Events
    if let Some(ev) = event_logs {
        if let Some(summary) = ev.data.get("summary") {
            let bsod_count = summary.get("bsod_count").and_then(json_u64).unwrap_or(0);
            let unexpected = summary
                .get("unexpected_shutdowns")
                .and_then(json_u64)
                .unwrap_or(0);
            if bsod_count > 0 {
                let code = summary
                    .get("last_bugcheck_code")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                b.add(
                    AlertSeverity::Critical,
                    "bsod",
                    "bsod_recent",
                    format!("Tela azul detectada ({code})"),
                );
            } else if unexpected > 0 {
                b.add(
                    AlertSeverity::Warning,
                    "bsod",
                    "unexpected_shutdown",
                    format!("{unexpected} desligamento(s) inesperado(s) nos últimos 14 dias"),
                );
            }
        }
    }

    if let Some(os) = os_col {
        if let Some(build) = os.data.get("build").and_then(|v| v.as_str()) {
            if let Ok(build_n) = build.parse::<u32>() {
                if build_n < 19041 {
                    b.add(
                        AlertSeverity::Warning,
                        "updates",
                        "old_build",
                        format!("Build Windows antiga ({build})"),
                    );
                }
            }
        }
    }

    // Cadastro TI
    if let Some(a) = admin {
        if a.owner_name.as_deref().unwrap_or("").is_empty() {
            b.add(
                AlertSeverity::Info,
                "cadastro",
                "owner_missing",
                "Cadastro TI: responsável não preenchido",
            );
        }
        if a.ramal.as_deref().unwrap_or("").is_empty() {
            b.add(
                AlertSeverity::Info,
                "cadastro",
                "ramal_missing",
                "Cadastro TI: ramal não preenchido",
            );
        }
    }

    if machine.status == belarc_shared::MachineStatus::Offline {
        b.add(
            AlertSeverity::Info,
            "connectivity",
            "offline",
            "Agente sem comunicação — verificar serviço BelarcInventoryAgent",
        );
    }

    b.finish()
}

pub fn evaluate_compliance(
    machine: &MachineSummary,
    collectors: &[CollectorResult],
    config: &ServerConfig,
    admin: Option<&MachineAdmin>,
    profile: &belarc_shared::CompanyProfile,
) -> (u8, Vec<AlertRecord>) {
    let breakdown = compute_score_breakdown(machine, collectors);
    let alerts = build_alerts(machine, collectors, config, admin, profile);
    (breakdown.total, alerts)
}

pub fn alerts_to_markdown(alerts: &[AlertRecord]) -> String {
    if alerts.is_empty() {
        return "## Alertas ativos\n\nNenhum alerta ativo.\n\n".into();
    }
    let mut md = String::from("## Alertas ativos\n\n");
    for a in alerts {
        let sev = match a.severity {
            AlertSeverity::Critical => "Crítico",
            AlertSeverity::Warning => "Atenção",
            AlertSeverity::Info => "Info",
        };
        md.push_str(&format!("- **[{sev}]** {} — {}\n", a.category, a.message));
    }
    md.push('\n');
    md
}

#[cfg(test)]
mod tests {
    use super::*;
    use belarc_shared::MachineStatus;
    use chrono::Utc;

    fn machine() -> MachineSummary {
        MachineSummary {
            id: "test-id".into(),
            hostname: "PC-TEST".into(),
            serial: None,
            status: MachineStatus::Online,
            logged_user: None,
            ip_address: None,
            uptime_seconds: Some(86400 * 3),
            last_seen: Utc::now(),
            first_seen: Utc::now(),
            health_score: None,
        }
    }

    fn base_hw() -> CollectorResult {
        CollectorResult {
            name: "hardware".into(),
            version: "1".into(),
            data: serde_json::json!({
                "physical_disks": [{ "health_status": "Healthy", "media_type": "SSD" }],
                "logical_disks": [{ "letter": "C", "free_percent": 40.0 }],
                "ram": { "total_gb": 16.0 }
            }),
            hash: String::new(),
            duration_ms: 0,
            error: None,
        }
    }

    fn base_sec() -> CollectorResult {
        CollectorResult {
            name: "security".into(),
            version: "1".into(),
            data: serde_json::json!({
                "eset": { "installed": true, "service_running": true },
                "firewall": { "enabled": true }
            }),
            hash: String::new(),
            duration_ms: 0,
            error: None,
        }
    }

    #[test]
    fn root_expired_certs_do_not_lower_score() {
        let certs = CollectorResult {
            name: "certificates".into(),
            version: "1".into(),
            data: serde_json::json!({
                "expired": [{
                    "subject": "CN=Baltimore CyberTrust Root",
                    "issuer": "CN=Baltimore CyberTrust Root",
                    "store": "Cert:\\LocalMachine\\Root",
                    "has_private_key": false,
                    "thumbprint": "abc"
                }]
            }),
            hash: String::new(),
            duration_ms: 0,
            error: None,
        };
        let bd = compute_score_breakdown(&machine(), &[certs, base_hw(), base_sec()]);
        assert_eq!(bd.certificates, WEIGHT_CERTIFICATES);
        assert!(bd.total >= 75);
    }

    #[test]
    fn expired_ecnpj_lowers_cert_score_only() {
        let certs = CollectorResult {
            name: "certificates".into(),
            version: "1".into(),
            data: serde_json::json!({
                "expired": [{
                    "subject": "CN=FABRIKAM EQUIPAMENTOS LTDA:17211893000117, OU=RFB e-CNPJ A1, O=ICP-Brasil",
                    "store": "Cert:\\LocalMachine\\My",
                    "has_private_key": true,
                    "thumbprint": "xyz"
                }]
            }),
            hash: String::new(),
            duration_ms: 0,
            error: None,
        };
        let bd = compute_score_breakdown(&machine(), &[certs, base_hw(), base_sec()]);
        assert_eq!(
            bd.certificates,
            WEIGHT_CERTIFICATES.saturating_mul(40).saturating_div(100)
        );
        assert!(bd.total >= 90);
    }

    #[test]
    fn self_signed_does_not_generate_alert_or_score_hit() {
        let certs = CollectorResult {
            name: "certificates".into(),
            version: "1".into(),
            data: serde_json::json!({
                "expired": [{
                    "subject": "CN=projeto2-william",
                    "issuer": "CN=projeto2-william",
                    "store": "Cert:\\LocalMachine\\Root",
                    "has_private_key": true,
                    "thumbprint": "ss1",
                    "days_left": -10
                }]
            }),
            hash: String::new(),
            duration_ms: 0,
            error: None,
        };
        let bd = compute_score_breakdown(&machine(), &[certs.clone(), base_hw(), base_sec()]);
        assert_eq!(bd.certificates, WEIGHT_CERTIFICATES);
        let profile = belarc_shared::CompanyProfile::demo();
        let alerts = build_alerts(
            &machine(),
            &[certs, base_hw(), base_sec()],
            &ServerConfig::default(),
            None,
            &profile,
        );
        assert!(!alerts.iter().any(|a| a.category == "certificate"));
    }

    #[test]
    fn eset_active_firewall_off_minor_penalty() {
        let sec = CollectorResult {
            name: "security".into(),
            version: "1".into(),
            data: serde_json::json!({
                "eset": { "installed": true, "service_running": true },
                "firewall": { "enabled": false }
            }),
            hash: String::new(),
            duration_ms: 0,
            error: None,
        };
        let bd = compute_score_breakdown(&machine(), &[base_hw(), sec]);
        assert_eq!(bd.security, WEIGHT_SECURITY.saturating_sub(2));
    }
}
