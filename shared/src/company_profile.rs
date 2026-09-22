use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompanyProfile {
    pub company_name: String,
    pub project_tagline: String,
    pub erp_name: String,
    pub erp_server_ip: String,
    pub inventory_server_ip: String,
    pub antivirus_name: String,
    pub banking_app: BankingAppConfig,
    pub erp_branches: Vec<ErpBranchConfig>,
    pub ui: UiLabels,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BankingAppConfig {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErpBranchConfig {
    pub id: String,
    pub label: String,
    pub install_path: String,
    pub temp_paths: Vec<String>,
    #[serde(default)]
    pub process_patterns: Vec<String>,
    #[serde(default)]
    pub product_patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiLabels {
    pub erp_user: String,
    pub erp_access_reference: String,
    pub erp_offline_warning: String,
    pub freeze_prone_description: String,
    pub critical_apps_summary: String,
}

impl Default for CompanyProfile {
    fn default() -> Self {
        Self::demo()
    }
}

impl CompanyProfile {
    pub fn demo() -> Self {
        serde_json::from_str(include_str!("../../config/company-profile.json"))
            .expect("embedded company-profile.json must be valid")
    }

    pub fn load() -> Self {
        for path in candidate_paths() {
            if path.is_file() {
                if let Ok(raw) = std::fs::read_to_string(&path) {
                    if let Ok(profile) = serde_json::from_str(&raw) {
                        return profile;
                    }
                }
            }
        }
        Self::demo()
    }

    pub fn erp_branch_ids(&self) -> Vec<&str> {
        self.erp_branches.iter().map(|b| b.id.as_str()).collect()
    }
}

fn candidate_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(custom) = std::env::var("BELARC_COMPANY_PROFILE") {
        paths.push(PathBuf::from(custom));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            paths.push(dir.join("config").join("company-profile.json"));
            paths.push(dir.join("company-profile.json"));
        }
    }
    paths.push(PathBuf::from("config/company-profile.json"));
    paths
}

pub fn write_profile_json(path: &Path, profile: &CompanyProfile) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(profile)?;
    std::fs::write(path, raw)
}
