use std::collections::HashSet;

use belarc_shared::{
    incident_stable_id, AlertSeverity, CollectorResult, CompanyProfile, IncidentRecord,
};
use chrono::{DateTime, Utc};

use crate::freeze_prone_apps::{self, FreezeProneApp};

pub fn detect_incidents(
    machine_id: &str,
    collectors: &[CollectorResult],
    profile: &CompanyProfile,
    freeze_apps: &[FreezeProneApp],
) -> Vec<IncidentRecord> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    let performance = collectors.iter().find(|c| c.name == "performance");
    let event_logs = collectors.iter().find(|c| c.name == "event_logs");
    let hardware = collectors.iter().find(|c| c.name == "hardware");

    if let Some(perf) = performance {
        collect_performance_signals(machine_id, perf, &mut out, &mut seen, profile, freeze_apps);
    }
    if let Some(ev) = event_logs {
        collect_event_incidents(machine_id, ev, &mut out, &mut seen, freeze_apps);
    }
    if let Some(hw) = hardware {
        collect_hardware_incidents(machine_id, hw, &mut out, &mut seen);
    }

    out.sort_by(|a, b| b.observed_at.cmp(&a.observed_at));
    out
}

fn push_incident(
    out: &mut Vec<IncidentRecord>,
    seen: &mut HashSet<String>,
    machine_id: &str,
    category: &str,
    dedupe_key: &str,
    severity: AlertSeverity,
    title: &str,
    message: &str,
    metric_value: Option<f64>,
    threshold: Option<f64>,
    recommendation: Option<&str>,
    source: &str,
    observed_at: DateTime<Utc>,
) {
    let id = incident_stable_id(machine_id, category, dedupe_key);
    if !seen.insert(id.clone()) {
        return;
    }
    out.push(IncidentRecord {
        id,
        machine_id: machine_id.into(),
        category: category.into(),
        severity,
        title: title.into(),
        message: message.into(),
        metric_value,
        threshold,
        recommendation: recommendation.map(String::from),
        source_collector: source.into(),
        observed_at,
        first_seen: observed_at,
        last_seen: observed_at,
        occurrence_count: 1,
    });
}

fn collect_performance_signals(
    machine_id: &str,
    perf: &CollectorResult,
    out: &mut Vec<IncidentRecord>,
    seen: &mut HashSet<String>,
    profile: &CompanyProfile,
    freeze_apps: &[FreezeProneApp],
) {
    let collected_at = perf
        .data
        .get("collected_at")
        .and_then(|v| v.as_str())
        .and_then(parse_time)
        .unwrap_or_else(Utc::now);

    if let Some(signals) = perf.data.get("signals").and_then(|v| v.as_array()) {
        for sig in signals {
            let sig_type = sig
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let severity = match sig.get("severity").and_then(|v| v.as_str()) {
                Some("critical") => AlertSeverity::Critical,
                Some("warning") => AlertSeverity::Warning,
                _ => AlertSeverity::Info,
            };
            let message = sig
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or(sig_type);
            let title = incident_title(sig_type);
            let metric = sig.get("value").and_then(json_f64);
            let threshold = sig.get("threshold").and_then(json_f64);
            let rec: Option<String> = sig
                .get("recommendation")
                .and_then(|v| v.as_str())
                .map(String::from)
                .or_else(|| {
                    sig.get("app_id").and_then(|v| v.as_str()).and_then(|id| {
                        freeze_apps
                            .iter()
                            .find(|a| a.id == id)
                            .map(|a| a.remediation.clone())
                    })
                })
                .or_else(|| {
                    freeze_prone_apps::match_product(message, freeze_apps)
                        .map(|a| a.remediation.clone())
                })
                .or_else(|| {
                    if sig_type == "erp_unreachable" {
                        Some(freeze_prone_apps::erp_unreachable_remediation(profile))
                    } else {
                        None
                    }
                });
            let observed = sig
                .get("observed_at")
                .and_then(|v| v.as_str())
                .and_then(parse_time)
                .unwrap_or(collected_at);
            let event_key = sig
                .get("event_key")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_else(|| {
                    format!(
                        "{}|{}|{}",
                        observed.format("%Y-%m-%d"),
                        sig_type,
                        metric.map(|m| m.to_string()).unwrap_or_default()
                    )
                });

            push_incident(
                out,
                seen,
                machine_id,
                sig_type,
                &event_key,
                severity,
                title,
                message,
                metric,
                threshold,
                rec.as_deref(),
                "performance",
                observed,
            );
        }
    }
}

fn collect_event_incidents(
    machine_id: &str,
    ev: &CollectorResult,
    out: &mut Vec<IncidentRecord>,
    seen: &mut HashSet<String>,
    freeze_apps: &[FreezeProneApp],
) {
    if let Some(bsod) = ev.data.get("bsod_events").and_then(|v| v.as_array()) {
        for event in bsod {
            let category = event
                .get("category")
                .and_then(|v| v.as_str())
                .unwrap_or("SYSTEM_CRITICAL");
            let sig_type = match category {
                "BSOD_BUGCHECK" => "bsod",
                "KERNEL_POWER_UNEXPECTED" => "unexpected_shutdown",
                "UNEXPECTED_SHUTDOWN" => "unexpected_shutdown",
                _ => "system_critical",
            };
            if sig_type == "SHUTDOWN_INITIATED" || category == "SHUTDOWN_INITIATED" {
                continue;
            }
            let time = event.get("time").and_then(|v| v.as_str()).unwrap_or("");
            let summary = event
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("Evento crítico de sistema");
            let eid = event.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let observed = parse_time(time).unwrap_or_else(Utc::now);
            let dedupe = format!("{time}|{eid}");

            let (severity, rec) = if sig_type == "bsod" {
                (
                    AlertSeverity::Critical,
                    "Verificar minidump, drivers e temperatura. Reiniciar após atualizar drivers.",
                )
            } else {
                (
                    AlertSeverity::Warning,
                    "Verificar energia, temperatura e Event Viewer (ID 41/6008).",
                )
            };

            push_incident(
                out,
                seen,
                machine_id,
                sig_type,
                &dedupe,
                severity,
                incident_title(sig_type),
                summary,
                None,
                None,
                Some(rec),
                "event_logs",
                observed,
            );
        }
    }

    if let Some(app_err) = ev
        .data
        .get("summary")
        .and_then(|s| s.get("application_errors_count"))
        .and_then(|v| v.as_u64())
    {
        if app_err >= 30 {
            let day = Utc::now().format("%Y-%m-%d").to_string();
            push_incident(
                out,
                seen,
                machine_id,
                "app_errors_high",
                &format!("{day}|count"),
                AlertSeverity::Warning,
                "Muitos erros de aplicativos",
                &format!("{app_err} erros de aplicativos nos últimos 14 dias"),
                Some(app_err as f64),
                Some(30.0),
                Some("Abrir Event Viewer > Application; reinstalar apps com falha recorrente."),
                "event_logs",
                Utc::now(),
            );
        }
    }

    // Travamentos/falhas de apps criticos (Event Viewer 1000/1002)
    if let Some(critical) = ev
        .data
        .get("critical_app_events")
        .and_then(|v| v.as_array())
    {
        for event in critical {
            let category = event
                .get("category")
                .and_then(|v| v.as_str())
                .unwrap_or("known_app_crash");
            let app_label = event
                .get("app_label")
                .and_then(|v| v.as_str())
                .unwrap_or("App crítico");
            let message = event
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or(app_label);
            let detail = event.get("detail").and_then(|v| v.as_str()).unwrap_or("");
            let eid = event.get("event_id").and_then(|v| v.as_u64()).unwrap_or(0);
            let exc = event
                .get("exception_code")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let fault = event
                .get("fault_module")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let time = event.get("time").and_then(|v| v.as_str()).unwrap_or("");
            let observed = parse_time(time).unwrap_or_else(Utc::now);
            let dedupe = event
                .get("event_key")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_else(|| format!("{time}|{eid}|{app_label}"));

            let mut full_msg = message.to_string();
            if !exc.is_empty() {
                full_msg.push_str(&format!(" [cod {exc}]"));
            }
            if !fault.is_empty() {
                full_msg.push_str(&format!(" mod {fault}"));
            }
            if !detail.is_empty() && detail.len() <= 120 {
                full_msg.push_str(&format!(" — {detail}"));
            }

            let rec = event
                .get("remediation")
                .and_then(|v| v.as_str())
                .map(String::from)
                .or_else(|| {
                    event.get("app_id").and_then(|v| v.as_str()).and_then(|id| {
                        freeze_apps
                            .iter()
                            .find(|a| a.id == id)
                            .map(|a| a.remediation.clone())
                    })
                });

            push_incident(
                out,
                seen,
                machine_id,
                category,
                &dedupe,
                AlertSeverity::Warning,
                incident_title(category),
                &full_msg,
                Some(eid as f64),
                None,
                rec.as_deref(),
                "event_logs",
                observed,
            );
        }
    }

    // Erros de disco / SSD (System log)
    if let Some(disk_ev) = ev.data.get("disk_events").and_then(|v| v.as_array()) {
        for event in disk_ev.iter().take(15) {
            let time = event.get("time").and_then(|v| v.as_str()).unwrap_or("");
            let eid = event.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let summary = event
                .get("summary")
                .and_then(|v| v.as_str())
                .or_else(|| event.get("message").and_then(|v| v.as_str()))
                .unwrap_or("Erro de disco");
            let observed = parse_time(time).unwrap_or_else(Utc::now);
            let dedupe = format!("{time}|{eid}");

            push_incident(
                out,
                seen,
                machine_id,
                "disk_error",
                &dedupe,
                AlertSeverity::Critical,
                "Erro de disco/SSD",
                summary,
                Some(eid as f64),
                None,
                Some("Verificar SMART, cabos SATA/NVMe e backup imediato. Substituir disco se recorrente."),
                "event_logs",
                observed,
            );
        }
    }

    // WHEA hardware / temperatura
    if let Some(whea) = ev.data.get("whea_events").and_then(|v| v.as_array()) {
        for event in whea.iter().take(10) {
            let time = event.get("time").and_then(|v| v.as_str()).unwrap_or("");
            let eid = event.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let summary = event
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("Evento WHEA");
            let observed = parse_time(time).unwrap_or_else(Utc::now);
            let dedupe = format!("whea|{time}|{eid}");

            push_incident(
                out,
                seen,
                machine_id,
                "whea_hardware",
                &dedupe,
                AlertSeverity::Warning,
                "Hardware / temperatura (WHEA)",
                summary,
                Some(eid as f64),
                None,
                Some("Verificar ventilação, cooler, pasta térmica e memória RAM."),
                "event_logs",
                observed,
            );
        }
    }
}

fn collect_hardware_incidents(
    machine_id: &str,
    hw: &CollectorResult,
    out: &mut Vec<IncidentRecord>,
    seen: &mut HashSet<String>,
) {
    if let Some(disks) = hw.data.get("physical_disks").and_then(|v| v.as_array()) {
        for disk in disks {
            let model = disk
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("disco");
            if let Some(temp) = disk
                .get("reliability")
                .and_then(|r| r.get("temperature_celsius"))
                .and_then(json_f64)
            {
                if temp >= 80.0 {
                    let sev = if temp >= 90.0 {
                        AlertSeverity::Critical
                    } else {
                        AlertSeverity::Warning
                    };
                    let day = Utc::now().format("%Y-%m-%d").to_string();
                    push_incident(
                        out,
                        seen,
                        machine_id,
                        "temp_high",
                        &format!("{model}|{day}"),
                        sev,
                        "Temperatura do disco elevada",
                        &format!("{model}: {temp:.0}°C"),
                        Some(temp),
                        Some(80.0),
                        Some("Melhorar ventilação; verificar cooler e pasta térmica do SSD/HDD."),
                        "hardware",
                        Utc::now(),
                    );
                }
            }
            let health = disk
                .get("health_status")
                .or_else(|| disk.get("status"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if health.eq_ignore_ascii_case("Unhealthy") || health.eq_ignore_ascii_case("Warning") {
                push_incident(
                    out,
                    seen,
                    machine_id,
                    "smart_unhealthy",
                    model,
                    AlertSeverity::Critical,
                    "Disco SMART com alerta",
                    &format!("{model}: status {health}"),
                    None,
                    None,
                    Some("Backup imediato e substituição do disco planejada."),
                    "hardware",
                    Utc::now(),
                );
            }
        }
    }
}

fn incident_title(category: &str) -> &str {
    match category {
        "cpu_high" => "CPU alta",
        "cpu_elevated" => "CPU elevada",
        "memory_high" => "Memória alta",
        "disk_critical" => "Disco quase cheio",
        "disk_low" => "Pouco espaço em disco",
        "disk_saturation" => "Disco saturado (100%)",
        "disk_queue" => "Fila de disco alta",
        "temp_high" => "Temperatura alta",
        "temp_critical" => "Temperatura crítica",
        "app_crash" => "Aplicativo travou/falhou",
        "app_hang" => "Aplicativo travou (hang)",
        "erp_unreachable" => "Servidor ERP inalcancavel",
        "known_app_crash" => "Programa critico falhou",
        "known_app_hang" => "Programa critico travou",
        "bugcheck" => "BugCheck / tela azul",
        "system_failure" => "Falha do Windows",
        "bsod" => "Tela azul (BSOD)",
        "unexpected_shutdown" => "Desligamento inesperado",
        "system_critical" => "Evento crítico",
        "app_errors_high" => "Erros de apps elevados",
        "disk_error" => "Erro disco/SSD",
        "whea_hardware" => "Hardware WHEA",
        "smart_unhealthy" => "Disco SMART",
        _ => "Incidente de performance",
    }
}

fn parse_time(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&Utc))
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
                .ok()
                .map(|n| n.and_utc())
        })
}

fn json_f64(v: &serde_json::Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_u64().map(|n| n as f64))
        .or_else(|| v.as_i64().map(|n| n as f64))
}
