use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatPayload {
    pub agent_token: String,
    pub hostname: String,
    pub serial: Option<String>,
    pub uuid: Option<String>,
    pub mac_primary: Option<String>,
    pub logged_user: Option<String>,
    pub ip_address: Option<String>,
    pub uptime_seconds: u64,
    pub last_boot: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterPayload {
    pub agent_token: String,
    pub hostname: String,
    pub serial: Option<String>,
    pub machine_uuid: Option<String>,
    pub mac_primary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryPayload {
    pub agent_token: String,
    pub hostname: String,
    pub tier: CollectionTier,
    pub collectors: Vec<CollectorResult>,
    pub collected_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CollectionTier {
    T0,
    T1,
    T2,
    T3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectorResult {
    pub name: String,
    pub version: String,
    pub data: serde_json::Value,
    pub hash: String,
    pub duration_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineSummary {
    pub id: String,
    pub hostname: String,
    pub serial: Option<String>,
    pub status: MachineStatus,
    pub logged_user: Option<String>,
    pub ip_address: Option<String>,
    pub uptime_seconds: Option<u64>,
    pub last_seen: DateTime<Utc>,
    pub first_seen: DateTime<Utc>,
    pub health_score: Option<u8>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MachineStatus {
    Online,
    Offline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRecord {
    pub id: String,
    pub machine_id: String,
    pub hostname: String,
    pub severity: AlertSeverity,
    pub category: String,
    pub message: String,
    pub created_at: DateTime<Utc>,
    pub resolved: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub blacklist_software: Vec<String>,
    pub acl_paths: Vec<String>,
    pub offline_threshold_minutes: u64,
    pub heartbeat_interval_seconds: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            blacklist_software: vec!["utorrent".into(), "bittorrent".into(), "teamviewer".into()],
            acl_paths: vec![],
            offline_threshold_minutes: 10,
            heartbeat_interval_seconds: 180,
        }
    }
}

pub fn hash_json(value: &serde_json::Value) -> String {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    let digest = Sha256::digest(bytes);
    format!("{:x}", digest)
}

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentRecord {
    pub id: String,
    pub machine_id: String,
    pub category: String,
    pub severity: AlertSeverity,
    pub title: String,
    pub message: String,
    pub metric_value: Option<f64>,
    pub threshold: Option<f64>,
    pub recommendation: Option<String>,
    pub source_collector: String,
    pub observed_at: DateTime<Utc>,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub occurrence_count: u32,
}

/// ID estável para registro de incidentes — histórico sem duplicar a cada coleta.
pub fn incident_stable_id(machine_id: &str, category: &str, key: &str) -> String {
    use sha2::{Digest, Sha256};
    let input = format!("inc|{machine_id}|{category}|{key}");
    let digest = Sha256::digest(input.as_bytes());
    format!("{:x}", digest)
}

/// ID estável para alertas — evita duplicatas a cada sincronização de inventário.
pub fn alert_stable_id(machine_id: &str, category: &str, key: &str) -> String {
    use sha2::{Digest, Sha256};
    let input = format!("{machine_id}|{category}|{key}");
    let digest = Sha256::digest(input.as_bytes());
    format!("{:x}", digest)
}

/// Identificador unico e estavel por hardware (evita PC duplicado ao gerar novo token).
pub fn machine_fingerprint(
    machine_uuid: Option<&str>,
    serial: Option<&str>,
    mac_primary: Option<&str>,
    hostname: &str,
) -> String {
    use sha2::{Digest, Sha256};
    let uuid = machine_uuid.unwrap_or("").trim().to_uppercase();
    let serial = serial.unwrap_or("").trim();
    let mac = mac_primary
        .unwrap_or("")
        .trim()
        .replace([':', '-'], "")
        .to_uppercase();

    let generic_serials = [
        "SYSTEM SERIAL NUMBER",
        "TO BE FILLED BY O.E.M.",
        "DEFAULT STRING",
        "",
    ];

    let key = if !uuid.is_empty() && uuid != "FFFFFFFF-FFFF-FFFF-FFFF-FFFFFFFFFFFF" {
        format!("uuid:{uuid}|mac:{mac}")
    } else if !mac.is_empty() {
        format!("mac:{mac}")
    } else if !generic_serials.contains(&serial.to_uppercase().as_str()) {
        format!("serial:{serial}")
    } else {
        format!("hostname:{hostname}|mac:{mac}")
    };

    format!("{:x}", Sha256::digest(key.as_bytes()))
}
