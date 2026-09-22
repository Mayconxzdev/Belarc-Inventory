use belarc_shared::{CollectorResult, MachineSummary};
use serde::{Deserialize, Serialize};

use crate::admin::{self, MachineAdmin};
use crate::standard_apps::{self, EsetInfo, LicenseKeysSummary, StandardAppStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineHighlight {
    #[serde(flatten)]
    pub machine: MachineSummary,
    pub lan_ip: Option<String>,
    pub lan_ips: Vec<String>,
    pub tailscale_installed: bool,
    pub tailscale_connected: bool,
    pub tailscale_ip: Option<String>,
    pub tailscale_dns: Option<String>,
    pub tailscale_startup: bool,
    pub anydesk_id: Option<String>,
    pub anydesk_running: bool,
    pub os_version: Option<String>,
    pub model: Option<String>,
    pub has_recent_bsod: bool,
    pub last_bsod_summary: Option<String>,
    pub last_bugcheck_code: Option<String>,
    pub system_errors_count: u32,
    pub windows_product_key: Option<String>,
    pub windows_key_partial: Option<String>,
    pub office_licenses: Vec<String>,
    pub thunderbird_emails: Vec<String>,
    pub thunderbird_profile: Option<String>,
    pub admin: MachineAdmin,
    pub display_email: Option<String>,
    pub standard_apps: Vec<StandardAppStatus>,
    pub standard_apps_installed: u32,
    pub license_keys: LicenseKeysSummary,
    pub eset_installed: bool,
    pub eset_product: Option<String>,
    pub eset_info: EsetInfo,
    pub banking_app_installed: bool,
    pub banking_app_version: Option<String>,
    pub folder_access: Vec<String>,
    pub installed_apps: Vec<String>,
}

pub fn extract_highlights(
    machine: MachineSummary,
    collectors: &[CollectorResult],
    admin: &MachineAdmin,
    profile: &belarc_shared::CompanyProfile,
) -> MachineHighlight {
    let mut h = MachineHighlight {
        machine,
        lan_ip: None,
        lan_ips: vec![],
        tailscale_installed: false,
        tailscale_connected: false,
        tailscale_ip: None,
        tailscale_dns: None,
        tailscale_startup: false,
        anydesk_id: None,
        anydesk_running: false,
        os_version: None,
        model: None,
        has_recent_bsod: false,
        last_bsod_summary: None,
        last_bugcheck_code: None,
        system_errors_count: 0,
        windows_product_key: None,
        windows_key_partial: None,
        office_licenses: vec![],
        thunderbird_emails: vec![],
        thunderbird_profile: None,
        admin: admin.clone(),
        display_email: None,
        standard_apps: vec![],
        standard_apps_installed: 0,
        license_keys: LicenseKeysSummary {
            windows_key_partial: None,
            windows_product_key: None,
            office_keys: vec![],
        },
        eset_installed: false,
        eset_product: None,
        eset_info: EsetInfo {
            installed: false,
            product_name: None,
            version: None,
            agent_version: None,
            install_path: None,
            service_running: false,
            real_time_active: None,
        },
        banking_app_installed: false,
        banking_app_version: None,
        folder_access: vec![],
        installed_apps: vec![],
    };

    for c in collectors {
        match c.name.as_str() {
            "network" => {
                h.lan_ip = json_str(&c.data, &["primary_lan_ip"]);
                if let Some(arr) = c.data.get("lan_ips").and_then(|v| v.as_array()) {
                    h.lan_ips = arr
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();
                }
                if h.lan_ip.is_none() {
                    h.lan_ip = h.lan_ips.first().cloned();
                }
                if let Some(smb) = c.data.get("smb_mappings").and_then(|v| v.as_array()) {
                    h.folder_access = smb
                        .iter()
                        .filter_map(|m| {
                            let local = m.get("local").and_then(|v| v.as_str());
                            let remote = m.get("remote").and_then(|v| v.as_str());
                            match (local, remote) {
                                (Some(l), Some(r)) => Some(format!("{l} → {r}")),
                                (_, Some(r)) => Some(r.to_string()),
                                (Some(l), None) => Some(l.to_string()),
                                _ => None,
                            }
                        })
                        .collect();
                }
            }
            "remote_access" => {
                if let Some(ts) = c.data.get("tailscale") {
                    h.tailscale_installed = ts
                        .get("installed")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    h.tailscale_connected = ts
                        .get("connected")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    h.tailscale_startup = ts
                        .get("startup_automatic")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    h.tailscale_dns = ts
                        .get("dns_name")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    if let Some(ips) = ts.get("tailscale_ips").and_then(|v| v.as_array()) {
                        let all: Vec<String> = ips
                            .iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect();
                        h.tailscale_ip = all
                            .iter()
                            .find(|ip| !ip.contains(':'))
                            .or_else(|| all.first())
                            .cloned();
                    }
                }
                if let Some(ad) = c.data.get("anydesk") {
                    h.anydesk_id = ad
                        .get("client_id")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    h.anydesk_running = ad
                        .get("service_running")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                }
            }
            "identity" => {
                h.model = c
                    .data
                    .get("model")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                if h.lan_ip.is_none() {
                    h.lan_ip = c
                        .data
                        .get("ip_primary")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                }
                if h.machine.logged_user.is_none() {
                    h.machine.logged_user = c
                        .data
                        .get("logged_user")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                }
                if h.machine.uptime_seconds.is_none() {
                    h.machine.uptime_seconds =
                        c.data.get("uptime_seconds").and_then(|v| v.as_u64());
                }
                if h.machine.ip_address.is_none() {
                    h.machine.ip_address = c
                        .data
                        .get("ip_primary")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                }
            }
            "hardware" => {
                if h.model.is_none() {
                    h.model = c
                        .data
                        .get("system")
                        .and_then(|s| s.get("model"))
                        .and_then(|v| v.as_str())
                        .map(String::from);
                }
            }
            "os" => {
                h.os_version = c
                    .data
                    .get("caption")
                    .and_then(|v| v.as_str())
                    .map(String::from);
            }
            "email" => {
                if let Some(summary) = c.data.get("thunderbird_summary") {
                    h.thunderbird_profile = summary
                        .get("default_profile")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    if let Some(arr) = summary.get("all_emails").and_then(|v| v.as_array()) {
                        h.thunderbird_emails = arr
                            .iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect();
                    }
                }
                if h.thunderbird_emails.is_empty() {
                    if let Some(profiles) = c.data.get("thunderbird").and_then(|v| v.as_array()) {
                        for p in profiles {
                            if let Some(emails) = p.get("emails").and_then(|v| v.as_array()) {
                                for e in emails {
                                    if let Some(s) = e.as_str() {
                                        h.thunderbird_emails.push(s.to_string());
                                    }
                                }
                            }
                        }
                        h.thunderbird_emails.sort();
                        h.thunderbird_emails.dedup();
                    }
                }
            }
            "licensing" => {
                if let Some(win) = c.data.get("windows") {
                    h.windows_product_key = win
                        .get("product_key")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    h.windows_key_partial = win
                        .get("product_key_partial")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                }
                if let Some(office) = c.data.get("office").and_then(|v| v.as_array()) {
                    h.office_licenses = office
                        .iter()
                        .filter_map(|o| {
                            let name = o.get("license_name").and_then(|v| v.as_str());
                            let id = o.get("product_id").and_then(|v| v.as_str());
                            let key = o.get("key_partial").and_then(|v| v.as_str());
                            match (name, id, key) {
                                (Some(n), _, Some(k)) => Some(format!("{n} (***-{k})")),
                                (Some(n), _, _) => Some(n.to_string()),
                                (_, Some(i), Some(k)) => Some(format!("{i} (***-{k})")),
                                (_, Some(i), _) => Some(i.to_string()),
                                _ => None,
                            }
                        })
                        .collect();
                }
            }
            "event_logs" => {
                if let Some(summary) = c.data.get("summary") {
                    h.has_recent_bsod = summary
                        .get("has_recent_bsod")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    h.last_bsod_summary = summary
                        .get("last_bsod_summary")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    h.last_bugcheck_code = summary
                        .get("last_bugcheck_code")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    h.system_errors_count = summary
                        .get("system_errors_count")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32;
                }
            }
            _ => {}
        }
    }

    h.license_keys = standard_apps::extract_license_keys(collectors);
    if h.windows_key_partial.is_none() {
        h.windows_key_partial = h.license_keys.windows_key_partial.clone();
    }
    if h.windows_product_key.is_none() {
        h.windows_product_key = h.license_keys.windows_product_key.clone();
    }

    h.standard_apps = standard_apps::detect_standard_apps(collectors, profile);
    h.standard_apps_installed = h.standard_apps.iter().filter(|a| a.installed).count() as u32;
    if let Some(banking) = h
        .standard_apps
        .iter()
        .find(|a| a.id == profile.banking_app.id)
    {
        h.banking_app_installed = banking.installed;
        h.banking_app_version = banking.version.clone();
    }
    h.installed_apps = h
        .standard_apps
        .iter()
        .filter(|a| a.installed)
        .map(|a| a.label.clone())
        .collect();
    h.eset_info = standard_apps::extract_eset_info(collectors);
    let (eset, eset_name) = standard_apps::eset_status(collectors);
    h.eset_installed = eset;
    h.eset_product = eset_name;
    h.display_email = admin::display_email(admin, &h.thunderbird_emails);

    h
}

fn json_str(data: &serde_json::Value, path: &[&str]) -> Option<String> {
    let mut cur = data;
    for key in path {
        cur = cur.get(*key)?;
    }
    cur.as_str().map(String::from)
}
