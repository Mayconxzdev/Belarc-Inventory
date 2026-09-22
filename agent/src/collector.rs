#![allow(
    unknown_lints,
    clippy::chunks_exact_to_as_chunks,
    clippy::manual_is_multiple_of,
    clippy::unnecessary_filter_map
)]

use std::process::Command;
use std::time::Instant;

use belarc_shared::{hash_json, CollectorResult, COLLECTOR_VERSION};
use thiserror::Error;

use crate::config::collectors_dir;

#[derive(Error, Debug)]
pub enum CollectorError {
    #[error("collector script not found: {0}")]
    NotFound(String),
    #[error("execution failed: {0}")]
    Execution(String),
    #[error("json parse failed: {0}")]
    Json(String),
}

/// PowerShell on Windows often writes UTF-16 LE to captured stdout.
fn decode_powershell_stdout(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return utf16_le_to_string(&bytes[2..]);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return utf16_be_to_string(&bytes[2..]);
    }
    // Heuristic: UTF-16 LE without BOM (null byte every second byte in ASCII output)
    if bytes.len() >= 4 && bytes.len() % 2 == 0 {
        let null_count = bytes
            .chunks_exact(2)
            .filter(|c| c[1] == 0 && c[0] != 0)
            .count();
        if null_count > bytes.len() / 8 {
            return utf16_le_to_string(bytes);
        }
    }
    String::from_utf8_lossy(bytes).into_owned()
}

fn utf16_le_to_string(bytes: &[u8]) -> String {
    let mut code_units = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        code_units.push(u16::from_le_bytes([chunk[0], chunk[1]]));
    }
    String::from_utf16_lossy(&code_units)
}

fn utf16_be_to_string(bytes: &[u8]) -> String {
    let mut code_units = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        code_units.push(u16::from_be_bytes([chunk[0], chunk[1]]));
    }
    String::from_utf16_lossy(&code_units)
}

pub fn run_collector(name: &str) -> Result<CollectorResult, CollectorError> {
    let start = Instant::now();
    let script_path = collectors_dir().join(format!("{name}.ps1"));

    if !script_path.exists() {
        return Err(CollectorError::NotFound(name.into()));
    }

    // Grava JSON em arquivo UTF-8 (evita corrupcao UTF-16 do stdout do PowerShell no Windows)
    let out_file =
        std::env::temp_dir().join(format!("belarc-{}-{}.json", name, uuid::Uuid::new_v4()));
    let script_escaped = script_path.to_string_lossy().replace('\'', "''");
    let out_escaped = out_file.to_string_lossy().replace('\'', "''");
    let ps_command = format!(
        "$OutputEncoding = [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); \
         $out = & '{script_escaped}' | Out-String; \
         [System.IO.File]::WriteAllText('{out_escaped}', $out.Trim(), [System.Text.UTF8Encoding]::new($false))"
    );

    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &ps_command,
        ])
        .output()
        .map_err(|e| CollectorError::Execution(e.to_string()))?;

    let duration_ms = start.elapsed().as_millis() as u64;

    if !output.status.success() {
        let stderr = decode_powershell_stdout(&output.stderr);
        let _ = std::fs::remove_file(&out_file);
        return Ok(CollectorResult {
            name: name.into(),
            version: COLLECTOR_VERSION.into(),
            data: serde_json::json!({}),
            hash: hash_json(&serde_json::json!({})),
            duration_ms,
            error: Some(stderr.to_string()),
        });
    }

    let stdout = std::fs::read_to_string(&out_file)
        .unwrap_or_else(|_| decode_powershell_stdout(&output.stdout));
    let _ = std::fs::remove_file(&out_file);

    let data: serde_json::Value = serde_json::from_str(stdout.trim()).map_err(|e| {
        CollectorError::Json(format!(
            "{e}: {}",
            stdout.chars().take(200).collect::<String>()
        ))
    })?;

    let hash = hash_json(&data);

    Ok(CollectorResult {
        name: name.into(),
        version: COLLECTOR_VERSION.into(),
        data,
        hash,
        duration_ms,
        error: None,
    })
}

pub fn collectors_for_tier(tier: &str) -> Vec<&'static str> {
    match tier {
        "t1" => vec![
            "identity",
            "os",
            "hardware",
            "peripherals",
            "network",
            "remote_access",
            "logins",
            "event_logs",
            "software",
            "licensing",
            "certificates",
            "security",
            "email",
            "permissions",
            "compliance",
            "runtime",
            "performance",
            "repair_status",
        ],
        "t2" => vec![
            "identity",
            "os",
            "hardware",
            "network",
            "remote_access",
            "logins",
            "event_logs",
            "software",
            "licensing",
            "certificates",
            "security",
            "email",
            "peripherals",
            "permissions",
            "compliance",
            "runtime",
            "performance",
            "repair_status",
        ],
        "t3" => vec![
            "identity",
            "os",
            "hardware",
            "network",
            "remote_access",
            "logins",
            "event_logs",
            "software",
            "licensing",
            "certificates",
            "security",
            "email",
            "peripherals",
            "permissions",
            "compliance",
            "runtime",
            "performance",
        ],
        _ => vec!["identity"],
    }
}

pub fn run_tier(tier: &str) -> Vec<CollectorResult> {
    collectors_for_tier(tier)
        .into_iter()
        .filter_map(|name| match run_collector(name) {
            Ok(result) => Some(result),
            Err(e) => {
                tracing::warn!("collector {name} failed: {e}");
                Some(CollectorResult {
                    name: name.into(),
                    version: COLLECTOR_VERSION.into(),
                    data: serde_json::json!({}),
                    hash: hash_json(&serde_json::json!({})),
                    duration_ms: 0,
                    error: Some(e.to_string()),
                })
            }
        })
        .collect()
}

pub fn build_heartbeat_from_identity(
    identity: &CollectorResult,
) -> belarc_shared::HeartbeatPayload {
    let d = &identity.data;
    belarc_shared::HeartbeatPayload {
        agent_token: String::new(),
        hostname: d
            .get("hostname")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .into(),
        serial: d.get("serial").and_then(|v| v.as_str()).map(String::from),
        uuid: d
            .get("machine_uuid")
            .and_then(|v| v.as_str())
            .map(String::from),
        mac_primary: d
            .get("mac_primary")
            .and_then(|v| v.as_str())
            .map(String::from),
        logged_user: d
            .get("logged_user")
            .and_then(|v| v.as_str())
            .map(String::from),
        ip_address: d
            .get("ip_primary")
            .and_then(|v| v.as_str())
            .map(String::from),
        uptime_seconds: d
            .get("uptime_seconds")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        last_boot: None,
    }
}

pub fn build_register_from_identity(
    identity: &CollectorResult,
    token: &str,
) -> belarc_shared::RegisterPayload {
    let d = &identity.data;
    belarc_shared::RegisterPayload {
        agent_token: token.into(),
        hostname: d
            .get("hostname")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .into(),
        serial: d.get("serial").and_then(|v| v.as_str()).map(String::from),
        machine_uuid: d
            .get("machine_uuid")
            .and_then(|v| v.as_str())
            .map(String::from),
        mac_primary: d
            .get("mac_primary")
            .and_then(|v| v.as_str())
            .map(String::from),
    }
}

pub fn filter_changed(
    results: Vec<CollectorResult>,
    cache: &crate::cache::LocalCache,
) -> Vec<CollectorResult> {
    results
        .into_iter()
        .filter(|r| {
            if r.error.is_some() {
                return true;
            }
            match cache.get_hash(&r.name) {
                Ok(Some(h)) => h != r.hash,
                _ => true,
            }
        })
        .collect()
}

pub fn update_cache_hashes(results: &[CollectorResult], cache: &crate::cache::LocalCache) {
    for r in results {
        if r.error.is_none() {
            let _ = cache.set_hash(&r.name, &r.hash);
        }
    }
}
