//! Gera Markdown pronto para colar em outra IA (diagnostico + reparo + comparacao).

use belarc_shared::{AlertRecord, CollectorResult, IncidentRecord, MachineSummary};

use crate::admin::MachineAdmin;
use crate::compliance::{compute_score_breakdown, score_band, ScoreBreakdown};
use crate::maintenance::maintenance_playbook;
use belarc_shared::CompanyProfile;

use crate::freeze_prone_apps::FreezeProneApp;

pub fn generate_compliance_prompt_md(
    machine: &MachineSummary,
    collectors: &[CollectorResult],
    alerts: &[AlertRecord],
    incidents: &[IncidentRecord],
    admin: Option<&MachineAdmin>,
    profile: &CompanyProfile,
    freeze_apps: &[FreezeProneApp],
) -> String {
    let breakdown = compute_score_breakdown(machine, collectors);
    let playbook = maintenance_playbook(profile, freeze_apps);
    let repair = collectors.iter().find(|c| c.name == "repair_status");
    let performance = collectors.iter().find(|c| c.name == "performance");
    let event_logs = collectors.iter().find(|c| c.name == "event_logs");

    let mut md = String::new();
    md.push_str("# Belarc Inventory — Prompt de diagnóstico TI\n\n");
    md.push_str("_Cole este documento em uma IA (ChatGPT, Claude, etc.) para analise completa, plano de reparo e verificacao de melhorias._\n\n");
    md.push_str("---\n\n");
    md.push_str("## Instrucoes para a IA\n\n");
    md.push_str("Voce e um assistente de TI corporativa (Windows 10/11, rede LAN, ERP OperationsSuite, ESET, apps de engenharia).\n\n");
    md.push_str("Analise **todos** os dados abaixo e responda:\n\n");
    md.push_str("1. **Diagnostico** — causas provaveis dos problemas (OperationsSuite, disco, certificado, rede).\n");
    md.push_str("2. **Prioridade** — o que corrigir primeiro (P0/P1/P2).\n");
    md.push_str(
        "3. **Comandos** — PowerShell/CMD adicionais alem dos ja executados pelo Belarc.\n",
    );
    md.push_str("4. **Verificacao** — como confirmar se o reparo automatico funcionou (compare metricas antes/depois).\n");
    md.push_str("5. **O que falta** — tarefas pendentes, instalacoes ausentes, politicas TI.\n");
    md.push_str("6. **Preventivo** — evitar recorrencia (OperationsSuite na LAN 192.0.2.25, SSD, temp).\n\n");
    md.push_str("---\n\n");

    md.push_str("## Identificacao do PC\n\n");
    md.push_str(&format!("- **Hostname:** {}\n", machine.hostname));
    if let Some(u) = &machine.logged_user {
        md.push_str(&format!("- **Usuario:** {u}\n"));
    }
    if let Some(ip) = &machine.ip_address {
        md.push_str(&format!("- **IP LAN:** {ip}\n"));
    }
    md.push_str(&format!("- **Status agente:** {:?}\n", machine.status));
    md.push_str(&format!(
        "- **Ultimo visto:** {}\n",
        machine.last_seen.format("%Y-%m-%d %H:%M UTC")
    ));
    if let Some(a) = admin {
        if let Some(n) = &a.owner_name {
            md.push_str(&format!("- **Responsavel TI:** {n}\n"));
        }
        if let Some(e) = &a.primary_email {
            md.push_str(&format!("- **E-mail:** {e}\n"));
        }
        if let Some(n) = &a.maintenance_notes {
            md.push_str(&format!("- **Notas manutencao:** {n}\n"));
        }
    }
    md.push('\n');

    md.push_str(&breakdown_prompt_section(&breakdown));
    md.push_str(&alerts_prompt_section(alerts));
    md.push_str(&incidents_prompt_section(incidents));
    md.push_str(&repair_prompt_section(repair));
    md.push_str(&performance_prompt_section(performance, event_logs));
    md.push_str(&playbook_prompt_section(&playbook));
    md.push_str(&todo_prompt_section(
        &breakdown, alerts, incidents, collectors, profile,
    ));

    md.push_str("\n---\n\n");
    md.push_str(&format!(
        "_Gerado em {} | Belarc Inventory | Empresa: {} | ERP: {} @ {}_\n",
        chrono::Utc::now().format("%Y-%m-%d %H:%M UTC"),
        profile.company_name,
        profile.erp_name,
        profile.erp_server_ip
    ));

    md
}

fn breakdown_prompt_section(bd: &ScoreBreakdown) -> String {
    let mut s = String::from("## Conformidade TI\n\n");
    s.push_str(&format!("**Score: {}% — {}**\n\n", bd.total, bd.band));
    s.push_str("| Categoria | Pontos | Max |\n|-----------|-------:|----:|\n");
    s.push_str(&format!("| SMART | {} | 30 |\n", bd.smart_disk));
    s.push_str(&format!("| Seguranca | {} | 25 |\n", bd.security));
    s.push_str(&format!(
        "| Windows Update | {} | 15 |\n",
        bd.windows_update
    ));
    s.push_str(&format!("| Eventos | {} | 10 |\n", bd.critical_events));
    s.push_str(&format!("| Disco livre | {} | 10 |\n", bd.disk_free));
    s.push_str(&format!("| Temperatura | {} | 5 |\n", bd.temperature));
    s.push_str(&format!("| Certificados | {} | 5 |\n", bd.certificates));
    if !bd.notes.is_empty() {
        s.push_str("\n**Observacoes do score:**\n");
        for n in &bd.notes {
            s.push_str(&format!("- {n}\n"));
        }
    }
    s.push('\n');
    s
}

fn alerts_prompt_section(alerts: &[AlertRecord]) -> String {
    let mut s = String::from("## Alertas ativos\n\n");
    if alerts.is_empty() {
        s.push_str("Nenhum alerta ativo.\n\n");
        return s;
    }
    for a in alerts {
        s.push_str(&format!(
            "- **[{:?}] {:?}** — {}\n",
            a.severity, a.category, a.message
        ));
    }
    s.push('\n');
    s
}

fn incidents_prompt_section(incidents: &[IncidentRecord]) -> String {
    let mut s = String::from("## Registro de incidentes (recentes)\n\n");
    if incidents.is_empty() {
        s.push_str("Nenhum incidente registrado.\n\n");
        return s;
    }
    for inc in incidents.iter().take(25) {
        s.push_str(&format!(
            "- **{}** ({}) — {} | ocorrencias: {} | ultima: {}\n",
            inc.title,
            inc.category,
            inc.message,
            inc.occurrence_count,
            inc.last_seen.format("%Y-%m-%d %H:%M")
        ));
        if let Some(r) = &inc.recommendation {
            s.push_str(&format!("  - Acao: {r}\n"));
        }
    }
    s.push('\n');
    s
}

fn repair_prompt_section(repair: Option<&CollectorResult>) -> String {
    let mut s = String::from("## Reparo automatico Belarc\n\n");
    let Some(r) = repair else {
        s.push_str("_Coletor repair_status indisponivel — reparo automatico pode nao estar configurado._\n\n");
        s.push_str("Para ativar no PC: `agendar-reparo-automatico.ps1` (Admin) + copiar `reparo-automatico.ps1`.\n\n");
        return s;
    };

    let task = r
        .data
        .get("scheduled_task_installed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    s.push_str(&format!(
        "- **Tarefa oculta BelarcInventoryRepair:** {}\n",
        if task { "instalada" } else { "NAO instalada" }
    ));
    if let Some(p) = r.data.get("pending") {
        if !p.is_null() {
            s.push_str("- **Fila pendente:** SIM — reparo aguardando execucao\n");
            if let Some(reasons) = p.get("reasons").and_then(|v| v.as_array()) {
                s.push_str("- **Motivos:** ");
                for reason in reasons {
                    if let Some(x) = reason.as_str() {
                        s.push_str(&format!("{x}, "));
                    }
                }
                s.push('\n');
            }
        } else {
            s.push_str("- **Fila pendente:** nao\n");
        }
    }

    if let Some(comps) = r.data.get("comparisons").and_then(|v| v.as_array()) {
        if !comps.is_empty() {
            s.push_str("\n### Historico de comparacao (antes vs depois)\n\n");
            for c in comps.iter().take(8) {
                let id = c.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                let prof = c.get("profile").and_then(|v| v.as_str()).unwrap_or("?");
                let improved = c.get("improved").and_then(|v| v.as_bool()).unwrap_or(false);
                let finished = c.get("finished_at").and_then(|v| v.as_str()).unwrap_or("?");
                s.push_str(&format!(
                    "- **{finished}** | perfil `{prof}` | melhorou: **{}** | id `{id}`\n",
                    if improved { "SIM" } else { "NAO" }
                ));
                if let Some(delta) = c.get("delta") {
                    if let Some(d) = delta.get("disk_free_min_delta").and_then(|v| v.as_f64()) {
                        s.push_str(&format!("  - Disco livre (min): {:+.1}%\n", d));
                    }
                    if let Some(d) = delta.get("disk_time_delta").and_then(|v| v.as_f64()) {
                        s.push_str(&format!("  - Disk Time: {:+.1}%\n", d));
                    }
                }
                if let Some(reasons) = c.get("reasons").and_then(|v| v.as_array()) {
                    let rs: Vec<_> = reasons.iter().filter_map(|x| x.as_str()).collect();
                    if !rs.is_empty() {
                        s.push_str(&format!("  - Motivos: {}\n", rs.join(", ")));
                    }
                }
            }
        } else {
            s.push_str("\n_Nenhum reparo automatico executado ainda._\n");
        }
    }

    if let Some(tail) = r
        .data
        .get("maintenance_log_tail")
        .and_then(|v| v.as_array())
    {
        if !tail.is_empty() {
            s.push_str("\n### Ultimas linhas maintenance.log\n\n```\n");
            for line in tail.iter().take(12) {
                if let Some(l) = line.as_str() {
                    s.push_str(l);
                    s.push('\n');
                }
            }
            s.push_str("```\n");
        }
    }
    s.push('\n');
    s
}

fn performance_prompt_section(
    perf: Option<&CollectorResult>,
    ev: Option<&CollectorResult>,
) -> String {
    let mut s = String::from("## Performance e apps criticos\n\n");
    if let Some(p) = perf {
        if let Some(cpu) = p.data.pointer("/cpu/load_percent") {
            s.push_str(&format!("- CPU: {}%\n", fmt_val(cpu)));
        }
        if let Some(mem) = p.data.pointer("/memory/used_percent") {
            s.push_str(&format!("- RAM: {}%\n", fmt_val(mem)));
        }
        if let Some(d) = p.data.pointer("/physical_disk_io/peak_disk_time_percent") {
            s.push_str(&format!("- Disco I/O pico: {}%\n", fmt_val(d)));
        }
        if let Some(erp) = p.data.get("erp_server") {
            let ok = erp.get("reachable").and_then(|v| v.as_bool());
            let ip = erp.get("ip").and_then(|v| v.as_str()).unwrap_or("?");
            s.push_str(&format!(
                "- ERP {}: {}\n",
                ip,
                match ok {
                    Some(true) => "online",
                    Some(false) => "OFFLINE",
                    _ => "?",
                }
            ));
        }
        if let Some(sum) = p.data.get("summary") {
            if let Some(q) = sum.get("auto_repair_queued").and_then(|v| v.as_bool()) {
                if q {
                    s.push_str("- **Reparo enfileirado nesta coleta:** sim\n");
                }
            }
        }
    }
    if let Some(e) = ev {
        if let Some(sum) = e.data.get("summary") {
            if let Some(n) = sum.get("critical_app_events_count") {
                s.push_str(&format!("- Eventos apps criticos (14d): {}\n", fmt_val(n)));
            }
            if let Some(n) = sum.get("critical_app_events_today") {
                s.push_str(&format!("- Eventos apps criticos HOJE: {}\n", fmt_val(n)));
            }
        }
    }
    s.push('\n');
    s
}

fn playbook_prompt_section(pb: &crate::maintenance::MaintenancePlaybook) -> String {
    let mut s = String::from("## Comandos de manutencao disponiveis\n\n");
    s.push_str("O Belarc pode executar automaticamente (tarefa oculta) ou manualmente:\n\n");
    for prof in &pb.profiles {
        s.push_str(&format!(
            "- **{}** — {} — `{}`\n",
            prof.label, prof.description, prof.command
        ));
    }
    s.push_str("\n**Se ERRO detectado**, o agente enfileira reparo com perfil:\n");
    s.push_str("- OperationsSuite/Fusion/AutoCAD/Libre travando → `AppsCriticos`\n");
    s.push_str("- Disco cheio → `Completo`\n");
    s.push_str("- Demais → `Rapido`\n\n");
    s
}

fn todo_prompt_section(
    bd: &ScoreBreakdown,
    alerts: &[AlertRecord],
    incidents: &[IncidentRecord],
    collectors: &[CollectorResult],
    profile: &CompanyProfile,
) -> String {
    let mut s = String::from("## Checklist — o que falta fazer\n\n");
    let mut items: Vec<String> = Vec::new();

    if bd.total < 75 {
        items.push(format!(
            "Elevar conformidade de {}% ({}) para >= 75%",
            bd.total, bd.band
        ));
    }
    for inc in incidents.iter().take(10) {
        if matches!(
            inc.category.as_str(),
            "known_app_hang" | "known_app_crash" | "erp_unreachable" | "disk_saturation"
        ) {
            items.push(format!("[{}] {}", inc.category, inc.message));
        }
    }
    for a in alerts {
        if a.severity == belarc_shared::AlertSeverity::Critical
            || a.severity == belarc_shared::AlertSeverity::Warning
        {
            items.push(format!("[alerta {}] {}", a.category, a.message));
        }
    }
    if let Some(sw) = collectors.iter().find(|c| c.name == "software") {
        if let Some(missing) = sw.data.get("missing_standard").and_then(|v| v.as_array()) {
            for app in missing {
                if let Some(l) = app.as_str() {
                    items.push(format!("Instalar software padrao: {l}"));
                }
            }
        }
    }
    items.push(format!(
        "Verificar ping ERP {} antes de usar OperationsSuite",
        profile.erp_server_ip
    ));

    if items.is_empty() {
        s.push_str("- PC em boa forma — manter monitoramento.\n");
    } else {
        for (i, item) in items.iter().enumerate() {
            s.push_str(&format!("{}. {item}\n", i + 1));
        }
    }
    s.push('\n');
    s
}

fn fmt_val(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::String(s) => s.clone(),
        _ => "—".into(),
    }
}

#[allow(dead_code)]
pub fn band_label(total: u8) -> &'static str {
    score_band(total)
}
