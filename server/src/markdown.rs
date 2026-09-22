use std::collections::HashSet;
use std::path::Path;

use belarc_shared::{AlertRecord, CollectorResult, IncidentRecord, MachineSummary};

use crate::compliance::{
    alerts_to_markdown, classify_certificate, compute_score_breakdown, CertClass,
};

pub fn generate_machine_report(
    reports_dir: &Path,
    machine: &MachineSummary,
    collectors: &[CollectorResult],
    alerts: &[AlertRecord],
    incidents: &[IncidentRecord],
) -> Result<String, std::io::Error> {
    let mut md = String::new();
    let breakdown = compute_score_breakdown(machine, collectors);

    md.push_str(&format!("# Inventário do PC - {}\n\n", machine.hostname));
    md.push_str(&format!(
        "_Gerado em: {} | Status: {:?} | Score: {}% ({})_\n\n",
        chrono::Utc::now().format("%Y-%m-%d %H:%M UTC"),
        machine.status,
        breakdown.total,
        breakdown.band
    ));

    md.push_str("## Identificação\n\n");
    md.push_str(&format!("- **Hostname:** {}\n", machine.hostname));
    if let Some(s) = &machine.serial {
        md.push_str(&format!("- **Serial:** {s}\n"));
    }
    if let Some(u) = &machine.logged_user {
        md.push_str(&format!("- **Usuário atual:** {u}\n"));
    }
    if let Some(ip) = &machine.ip_address {
        md.push_str(&format!("- **IP:** {ip}\n"));
    }
    if let Some(uptime) = machine.uptime_seconds {
        md.push_str(&format!(
            "- **Uptime:** {}h {}m\n",
            uptime / 3600,
            (uptime % 3600) / 60
        ));
    }
    md.push_str(&format!(
        "- **Último visto:** {}\n\n",
        machine.last_seen.format("%Y-%m-%d %H:%M")
    ));

    md.push_str(&breakdown.to_markdown());
    md.push_str(&alerts_to_markdown(alerts));
    md.push_str(&incidents_to_markdown(incidents));

    md.push_str("## Inventário (dados coletados)\n\n");
    md.push_str("_Camada de consulta — não altera o score diretamente._\n\n");

    for collector in collectors {
        if collector.name == "compliance" {
            continue;
        }
        let section_title = match collector.name.as_str() {
            "identity" => "Identificação detalhada",
            "os" => "Sistema operacional",
            "hardware" => "Hardware",
            "network" => "Rede",
            "remote_access" => "Acesso remoto",
            "software" => "Software",
            "licensing" => "Licenciamento",
            "certificates" => "Certificados",
            "security" => "Segurança",
            "email" => "E-mail",
            "peripherals" => "Periféricos",
            "permissions" => "Permissões",
            "logins" => "Usuários e logins",
            "event_logs" => "Eventos e BSOD",
            "runtime" => "Runtime (Docker, WSL, etc.)",
            "performance" => "Performance e recursos",
            other => other,
        };

        md.push_str(&format!("### {section_title}\n\n"));
        md.push_str(&collector_section_body(&collector.name, &collector.data));
        md.push('\n');
    }

    md.push_str("## Observações\n\n");
    md.push_str("- Relatório gerado automaticamente pelo Belarc Inventory.\n");
    md.push_str("- Inventário ≠ Alertas ≠ Conformidade TI — cada camada tem função distinta.\n");
    md.push_str("- Certificados raiz do Windows (Baltimore, Microsoft Root…) são históricos e não afetam o score.\n");

    let path = reports_dir.join(format!("{}.md", machine.hostname));
    std::fs::write(&path, &md)?;

    Ok(md)
}

fn collector_section_body(name: &str, data: &serde_json::Value) -> String {
    match name {
        "certificates" => certificates_to_markdown(data),
        "software" => software_to_markdown(data),
        "event_logs" => event_logs_to_markdown(data),
        "performance" => performance_to_markdown(data),
        _ => json_to_markdown(data, 0),
    }
}

fn certificates_to_markdown(data: &serde_json::Value) -> String {
    let total = data
        .get("total_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let mut md = format!("- **Total coletado:** {total}\n\n");

    let mut seen_thumb = HashSet::new();
    let mut unique_certs: Vec<&serde_json::Value> = Vec::new();
    for key in ["expired", "expiring_soon", "certificates"] {
        let Some(arr) = data.get(key).and_then(|v| v.as_array()) else {
            continue;
        };
        for cert in arr {
            let id = cert
                .get("thumbprint")
                .and_then(|t| t.as_str())
                .or_else(|| cert.get("subject").and_then(|s| s.as_str()))
                .unwrap_or("");
            if seen_thumb.insert(id.to_string()) {
                unique_certs.push(cert);
            }
        }
    }

    let mut self_signed = 0u32;
    let mut root = 0u32;
    // Agrupa por CN — uma linha por empresa, pior status primeiro
    let mut corp_by_cn: std::collections::HashMap<String, (i64, String, String)> =
        std::collections::HashMap::new();

    for cert in unique_certs {
        let class = classify_certificate(cert);
        match class {
            CertClass::Corporate => {
                let days = cert
                    .get("days_left")
                    .and_then(|d| d.as_i64())
                    .unwrap_or(9999);
                let subject = cert.get("subject").and_then(|s| s.as_str()).unwrap_or("?");
                let cn = subject
                    .strip_prefix("CN=")
                    .and_then(|s| s.split(',').next())
                    .unwrap_or(subject)
                    .to_string();
                let store = cert
                    .get("store")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .replace("Cert:\\", "");
                let status = match days {
                    d if d < 0 => format!("Expirado ({d}d)"),
                    d if d <= 30 => format!("Expira em {d}d"),
                    d => format!("Válido ({d}d)"),
                };
                corp_by_cn
                    .entry(cn)
                    .and_modify(|(worst, st, _)| {
                        if days < *worst {
                            *worst = days;
                            *st = status.clone();
                        }
                    })
                    .or_insert((days, status, store));
            }
            CertClass::SelfSigned => self_signed += 1,
            CertClass::RootHistorical => root += 1,
            CertClass::Other => {}
        }
    }

    let corp_exp = corp_by_cn.values().filter(|(d, _, _)| *d < 0).count() as u32;
    let corp_soon = corp_by_cn
        .values()
        .filter(|(d, _, _)| *d >= 0 && *d <= 30)
        .count() as u32;
    let mut corp_tuples: Vec<(String, i64, String, String)> = corp_by_cn
        .iter()
        .map(|(cn, (days, status, store))| (cn.clone(), *days, status.clone(), store.clone()))
        .collect();
    corp_tuples.sort_by_key(|(_, days, _, _)| *days);
    let corp_rows: Vec<String> = corp_tuples
        .into_iter()
        .take(15)
        .map(|(cn, _, status, store)| format!("| {cn} | {status} | {store} |"))
        .collect();

    md.push_str(&format!(
        "**Resumo:** Corporativos expirados: {corp_exp} · Expirando: {corp_soon} · Autoassinados: {self_signed} · Raiz/histórico: {root} (ignorados no score)\n\n"
    ));

    if !corp_rows.is_empty() {
        md.push_str("**Certificados corporativos (e-CNPJ / ICP-Brasil):**\n\n");
        md.push_str("| Nome | Validade | Store |\n");
        md.push_str("|------|----------|-------|\n");
        for row in corp_rows {
            md.push_str(&row);
            md.push('\n');
        }
        md.push('\n');
    } else {
        md.push_str("_Nenhum certificado corporativo detectado em My._\n\n");
    }

    md.push_str("_Lista completa de raízes Windows omitida — são inventário histórico._\n");
    md
}

fn software_to_markdown(data: &serde_json::Value) -> String {
    let count = data
        .get("program_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let mut md = format!("- **Programas instalados:** {count}\n\n");

    if let Some(office) = data.get("office_suite") {
        let name = office
            .get("display_name")
            .and_then(|v| v.as_str())
            .unwrap_or("—");
        let ver = office
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("—");
        md.push_str(&format!("- **Office:** {name} v{ver}\n"));
    }
    if let Some(browsers) = data.get("browsers").and_then(|v| v.as_array()) {
        for b in browsers {
            let name = b.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let ver = b.get("version").and_then(|v| v.as_str()).unwrap_or("—");
            md.push_str(&format!("- **Navegador:** {name} v{ver}\n"));
        }
    }
    md.push('\n');

    if let Some(apps) = data.get("standard_apps").and_then(|v| v.as_array()) {
        md.push_str("**Software padrão da empresa:**\n\n");
        md.push_str("| Programa | Status |\n");
        md.push_str("|----------|--------|\n");
        for a in apps {
            let label = a.get("label").and_then(|v| v.as_str()).unwrap_or("?");
            let ok = a
                .get("installed")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            md.push_str(&format!(
                "| {label} | {} |\n",
                if ok { "Instalado" } else { "Ausente" }
            ));
        }
        md.push('\n');
    }

    if let Some(programs) = data.get("programs").and_then(|v| v.as_array()) {
        let show = programs.len().min(40);
        md.push_str(&format!(
            "**Amostra de programas (primeiros {show} de {count}):**\n\n"
        ));
        for p in programs.iter().take(show) {
            let name = p.get("name").or_else(|| p.get("display_name"));
            let name = name.and_then(|v| v.as_str()).unwrap_or("?");
            let ver = p.get("version").and_then(|v| v.as_str()).unwrap_or("");
            md.push_str(&format!(
                "- {}{}\n",
                name,
                if ver.is_empty() {
                    String::new()
                } else {
                    format!(" v{ver}")
                }
            ));
        }
        if programs.len() > show {
            md.push_str(&format!(
                "\n_… e mais {} programas (ver dashboard JSON)._\n",
                programs.len() - show
            ));
        }
    }

    md
}

pub fn incidents_to_markdown(incidents: &[IncidentRecord]) -> String {
    if incidents.is_empty() {
        return String::new();
    }
    let mut md = String::from("## Registro de incidentes e performance\n\n");
    md.push_str("_Histórico persistente — travamentos, picos de CPU/disco, temperatura e falhas. Use para diagnóstico e plano de ação._\n\n");
    md.push_str("| Data | Tipo | Severidade | Detalhe | Valor | Ação recomendada | Vezes |\n");
    md.push_str("|------|------|------------|---------|-------|------------------|------:|\n");
    for inc in incidents.iter().take(40) {
        let sev = format!("{:?}", inc.severity);
        let val = inc
            .metric_value
            .map(|v| format!("{v:.1}"))
            .unwrap_or_else(|| "—".into());
        let rec = inc.recommendation.as_deref().unwrap_or("—");
        let date = inc.last_seen.format("%Y-%m-%d %H:%M");
        md.push_str(&format!(
            "| {date} | {} | {sev} | {} | {val} | {rec} | {} |\n",
            inc.title, inc.message, inc.occurrence_count
        ));
    }
    if incidents.len() > 40 {
        md.push_str(&format!(
            "\n_… e mais {} registros no dashboard._\n",
            incidents.len() - 40
        ));
    }
    md.push('\n');
    md
}

fn performance_to_markdown(data: &serde_json::Value) -> String {
    let mut md = String::new();
    if let Some(cpu) = data.get("cpu") {
        let load = cpu
            .get("load_percent")
            .map(format_scalar)
            .unwrap_or_else(|| "—".into());
        md.push_str(&format!("**CPU:** {load}% de uso na coleta\n"));
    }
    if let Some(mem) = data.get("memory") {
        let used = mem
            .get("used_percent")
            .map(format_scalar)
            .unwrap_or_else(|| "—".into());
        let total = mem
            .get("total_gb")
            .map(format_scalar)
            .unwrap_or_else(|| "—".into());
        md.push_str(&format!("**Memória:** {used}% usada ({total} GB total)\n"));
    }
    if let Some(io) = data.get("physical_disk_io") {
        let dt = io
            .get("peak_disk_time_percent")
            .or_else(|| io.get("total_disk_time_percent"))
            .map(format_scalar)
            .unwrap_or_else(|| "—".into());
        let q = io
            .get("avg_queue_length")
            .map(format_scalar)
            .unwrap_or_else(|| "—".into());
        md.push_str(&format!(
            "**Disco (I/O):** pico {dt}% Disk Time · fila {q}\n"
        ));
        if let Some(samples) = io.get("disk_time_samples").and_then(|v| v.as_array()) {
            if !samples.is_empty() {
                md.push_str("Amostras na coleta:\n");
                for s in samples {
                    let t = s.get("time").and_then(|v| v.as_str()).unwrap_or("—");
                    let v = s
                        .get("value")
                        .map(format_scalar)
                        .unwrap_or_else(|| "—".into());
                    md.push_str(&format!("- {t}: {v}%\n"));
                }
            }
        }
    }
    if let Some(temps) = data.get("temperatures").and_then(|v| v.as_array()) {
        if !temps.is_empty() {
            md.push_str("**Temperaturas:**\n");
            for t in temps.iter().take(6) {
                let c = t
                    .get("celsius")
                    .map(format_scalar)
                    .unwrap_or_else(|| "—".into());
                let src = t.get("source").and_then(|v| v.as_str()).unwrap_or("—");
                let inst = t.get("instance").and_then(|v| v.as_str()).unwrap_or("");
                md.push_str(&format!("- {src} {inst}: {c}°C\n"));
            }
        }
    }
    if let Some(top) = data.get("top_processes") {
        if let Some(cpu) = top.get("by_cpu").and_then(|v| v.as_array()) {
            if !cpu.is_empty() {
                md.push_str("\n**Top CPU:**\n");
                for p in cpu.iter().take(5) {
                    let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                    let mem = p.get("memory_mb").map(format_scalar).unwrap_or_default();
                    md.push_str(&format!("- {name} ({mem} MB RAM)\n"));
                }
            }
        }
    }
    if let Some(recs) = data.get("reliability_records").and_then(|v| v.as_array()) {
        let fails: Vec<_> = recs
            .iter()
            .filter(|r| {
                matches!(
                    r.get("record_type_id").and_then(|v| v.as_u64()),
                    Some(1) | Some(2) | Some(4)
                )
            })
            .take(8)
            .collect();
        if !fails.is_empty() {
            md.push_str("\n**Falhas recentes (Reliability Monitor):**\n");
            for r in fails {
                let t = r.get("time").and_then(|v| v.as_str()).unwrap_or("—");
                let prod = r.get("product").and_then(|v| v.as_str()).unwrap_or("—");
                let typ = r.get("record_type").and_then(|v| v.as_str()).unwrap_or("—");
                md.push_str(&format!("- {t} — {typ}: {prod}\n"));
            }
        }
    }
    md
}

fn event_logs_to_markdown(data: &serde_json::Value) -> String {
    let mut md = String::new();
    if let Some(sum) = data.get("summary") {
        md.push_str("**Resumo (14 dias):**\n\n");
        for (k, v) in sum.as_object().into_iter().flatten() {
            md.push_str(&format!("- **{k}:** {}\n", format_scalar(v)));
        }
        md.push('\n');
    }
    if let Some(tl) = data.get("daily_timeline") {
        if let Some(events) = tl.get("events").and_then(|v| v.as_array()) {
            if !events.is_empty() {
                let date = tl.get("date").and_then(|v| v.as_str()).unwrap_or("hoje");
                md.push_str(&format!("**Linha do tempo — {date}:**\n\n"));
                for ev in events.iter().take(25) {
                    let t = ev.get("time").and_then(|v| v.as_str()).unwrap_or("—");
                    let cat = ev.get("category").and_then(|v| v.as_str()).unwrap_or("—");
                    let title = ev
                        .get("title")
                        .and_then(|v| v.as_str())
                        .or_else(|| ev.get("message").and_then(|v| v.as_str()))
                        .unwrap_or("—");
                    md.push_str(&format!("- {t} — **{cat}** — {title}\n"));
                }
                md.push('\n');
            }
        }
    }
    if let Some(critical) = data.get("critical_app_events").and_then(|v| v.as_array()) {
        if !critical.is_empty() {
            md.push_str("**Apps críticos (OperationsSuite, CAD, LibreOffice):**\n\n");
            for ev in critical.iter().take(15) {
                let t = ev.get("time").and_then(|v| v.as_str()).unwrap_or("—");
                let app = ev.get("app_label").and_then(|v| v.as_str()).unwrap_or("—");
                let msg = ev.get("message").and_then(|v| v.as_str()).unwrap_or("—");
                let eid = ev
                    .get("event_id")
                    .map(format_scalar)
                    .unwrap_or_else(|| "—".into());
                md.push_str(&format!("- {t} — {app} (ID {eid}): {msg}\n"));
            }
            md.push('\n');
        }
    }
    if let Some(disk) = data.get("disk_events").and_then(|v| v.as_array()) {
        if !disk.is_empty() {
            md.push_str("**Erros de disco/SSD:**\n\n");
            for ev in disk.iter().take(10) {
                let t = ev.get("time").and_then(|v| v.as_str()).unwrap_or("—");
                let s = ev
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .or_else(|| ev.get("message").and_then(|v| v.as_str()))
                    .unwrap_or("—");
                md.push_str(&format!("- {t} — {s}\n"));
            }
            md.push('\n');
        }
    }
    if let Some(bsod) = data.get("bsod_events").and_then(|v| v.as_array()) {
        if !bsod.is_empty() {
            md.push_str("**Eventos de estabilidade:**\n\n");
            for ev in bsod.iter().take(10) {
                let t = ev.get("time").and_then(|v| v.as_str()).unwrap_or("—");
                let s = ev.get("summary").and_then(|v| v.as_str()).unwrap_or("—");
                md.push_str(&format!("- {t} — {s}\n"));
            }
            md.push('\n');
        }
    }
    md
}

fn json_to_markdown(value: &serde_json::Value, depth: usize) -> String {
    let indent = "  ".repeat(depth);
    let mut out = String::new();

    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map {
                match val {
                    serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                        out.push_str(&format!("{indent}- **{key}:**\n"));
                        out.push_str(&json_to_markdown(val, depth + 1));
                    }
                    _ => {
                        out.push_str(&format!("{indent}- **{key}:** {}\n", format_scalar(val)));
                    }
                }
            }
        }
        serde_json::Value::Array(arr) => {
            let limit = arr.len().min(50);
            for item in arr.iter().take(limit) {
                match item {
                    serde_json::Value::Object(obj) => {
                        let label = obj
                            .get("name")
                            .or_else(|| obj.get("hostname"))
                            .or_else(|| obj.get("display_name"))
                            .map(format_scalar)
                            .unwrap_or_else(|| "item".into());
                        out.push_str(&format!("{indent}- {label}\n"));
                        for (k, v) in obj {
                            if k != "name" && k != "hostname" && k != "display_name" {
                                out.push_str(&format!("{indent}  - {k}: {}\n", format_scalar(v)));
                            }
                        }
                    }
                    other => {
                        out.push_str(&format!("{indent}- {}\n", format_scalar(other)));
                    }
                }
            }
            if arr.len() > limit {
                out.push_str(&format!(
                    "{indent}_… mais {} itens omitidos._\n",
                    arr.len() - limit
                ));
            }
        }
        other => {
            out.push_str(&format!("{indent}{}\n", format_scalar(other)));
        }
    }

    out
}

fn format_scalar(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "—".into(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.clone(),
        _ => v.to_string(),
    }
}
