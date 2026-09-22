use std::time::Duration;

use crate::auth::{self, LoginRequest, LoginResponse, MeResponse, SESSION_HOURS};
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Json},
    routing::{get, patch, post},
    Router,
};
use belarc_shared::{
    HeartbeatPayload, InventoryPayload, MachineSummary, RegisterPayload, ServerConfig,
};
use serde::{Deserialize, Serialize};

use crate::admin::{MachineAdmin, UpdateMachineAdmin};
use crate::compliance::{compute_score_breakdown, evaluate_compliance, score_band};
use crate::compliance_prompt::generate_compliance_prompt_md;
use crate::incidents::detect_incidents;
use crate::markdown::generate_machine_report;
use crate::summary::{extract_highlights, MachineHighlight};
use crate::tickets::{
    self, AttachRequest, ChecklistCreateRequest, ChecklistPatchRequest, CloseTicketRequest,
    CommentRequest, CreateTicketRequest, RateTicketRequest, ReopenBody, ReopenDecideBody,
    ReopenRequestBody, Ticket, UpdateTicketRequest,
};
use crate::AppState;
use belarc_shared::AlertRecord;
use belarc_shared::IncidentRecord;

pub fn api_router(state: AppState) -> Router {
    let state_clone = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            let threshold = state_clone.config.offline_threshold_minutes;
            if let Err(e) = state_clone.db.update_offline_machines(threshold) {
                tracing::error!("offline sweep failed: {e}");
            }
        }
    });

    Router::new()
        .route("/api/health", get(health))
        .route("/api/register", post(register))
        .route("/api/heartbeat", post(heartbeat))
        .route("/api/inventory", post(inventory))
        .route("/api/machines", get(list_machines))
        .route("/api/dashboard", get(dashboard))
        .route("/api/machines/{id}", get(get_machine))
        .route("/api/machines/{id}/admin", patch(update_machine_admin))
        .route(
            "/api/machines/{id}/ticket-routing",
            patch(update_machine_ticket_routing),
        )
        .route(
            "/api/ticket-departments",
            get(list_ticket_departments).post(create_ticket_department),
        )
        .route(
            "/api/ticket-departments/{slug}",
            patch(rename_ticket_department).delete(archive_ticket_department),
        )
        .route("/api/machines/{id}/tickets", get(get_machine_tickets))
        .route("/api/machines/{id}/report", get(get_report))
        .route(
            "/api/machines/{id}/compliance-prompt",
            get(get_compliance_prompt),
        )
        .route("/api/machines/{id}/incidents", get(get_machine_incidents))
        .route("/api/incidents", get(list_incidents))
        .route("/api/maintenance/playbook", get(maintenance_playbook))
        .route("/api/alerts", get(list_alerts))
        .route("/api/alerts/{id}/resolve", post(resolve_alert))
        .route("/api/alerts/{id}/ticket", post(create_ticket_from_alert))
        .route("/api/fleet/health", get(fleet_health))
        .route("/api/config", get(get_config))
        .route("/api/company-profile", get(get_company_profile))
        .route("/api/tokens", post(create_token))
        .route("/api/admin/reset", post(admin_reset))
        .route("/api/auth/login", post(auth_login))
        .route("/api/auth/logout", post(auth_logout))
        .route("/api/auth/me", get(auth_me))
        .route("/api/portal/auth/login", post(portal_login))
        .route("/api/portal/auth/logout", post(portal_logout))
        .route("/api/portal/auth/me", get(portal_me))
        .route("/api/portal/machines", get(portal_machines))
        .route(
            "/api/portal/device-session",
            post(create_device_portal_session),
        )
        .route("/api/portal/device-routing", get(device_portal_routing))
        .route("/api/portal/users", post(create_portal_user))
        .route(
            "/api/portal/users/{id}/machines/{machine_id}",
            post(link_portal_user_machine),
        )
        .route("/api/tickets", get(list_tickets).post(create_ticket))
        .route("/api/tickets/mine", get(list_my_tickets))
        .route("/api/tickets/stats", get(ticket_stats))
        .route(
            "/api/tickets/{id}",
            get(get_ticket).patch(update_ticket).delete(delete_ticket),
        )
        .route("/api/tickets/{id}/attachments", post(attach_ticket_file))
        .route(
            "/api/tickets/{id}/attachments/{att_id}",
            get(download_ticket_attachment),
        )
        .route(
            "/api/tickets/{id}/attachments/{att_id}/nas",
            get(ticket_attachment_nas_path),
        )
        .route("/api/tickets/{id}/comments", post(add_ticket_comment))
        .route("/api/tickets/{id}/checklist", post(add_checklist_item))
        .route(
            "/api/tickets/{id}/checklist/{item_id}",
            axum::routing::patch(patch_checklist_item).delete(delete_checklist_item),
        )
        .route("/api/tickets/{id}/close", post(close_ticket))
        .route("/api/tickets/{id}/rate", post(rate_ticket))
        .route(
            "/api/tickets/{id}/reopen-request",
            post(request_ticket_reopen),
        )
        .route(
            "/api/tickets/{id}/reopen-decide",
            post(decide_ticket_reopen),
        )
        .route("/api/tickets/{id}/reopen", post(reopen_ticket))
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok", "service": "belarc-server" }))
}

async fn register(
    State(state): State<AppState>,
    Json(payload): Json<RegisterPayload>,
) -> Result<Json<RegisterResponse>, ApiError> {
    let id = state.db.register_agent(&payload)?;
    tracing::info!("registered agent: {} ({})", payload.hostname, id);
    Ok(Json(RegisterResponse {
        machine_id: id,
        message: "registered".into(),
    }))
}

async fn heartbeat(
    State(state): State<AppState>,
    Json(payload): Json<HeartbeatPayload>,
) -> Result<StatusCode, ApiError> {
    state.db.process_heartbeat(&payload)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn inventory(
    State(state): State<AppState>,
    Json(payload): Json<InventoryPayload>,
) -> Result<Json<InventoryResponse>, ApiError> {
    let changed = state.db.process_inventory(&payload)?;

    let machine_id = state.db.machine_id_by_token(&payload.agent_token)?;
    let machine = state.db.get_machine_by_id(&machine_id)?;
    let collectors = state.db.get_machine_collectors(&machine_id)?;

    let admin = state.db.get_machine_admin(&machine_id).ok();
    let (score, alerts) = evaluate_compliance(
        &machine,
        &collectors,
        &state.config,
        admin.as_ref(),
        &state.company_profile,
    );
    state.db.set_health_score(&machine_id, score)?;
    state.db.sync_machine_alerts(&machine_id, &alerts)?;

    let incidents = detect_incidents(
        &machine_id,
        &collectors,
        &state.company_profile,
        &state.freeze_prone_apps,
    );
    if let Err(e) = state.db.upsert_incidents(&incidents) {
        tracing::warn!("incident upsert failed for {}: {e}", machine.hostname);
    }
    let incident_history = state
        .db
        .list_machine_incidents(&machine_id, 80)
        .unwrap_or_default();

    if let Err(e) = generate_machine_report(
        &state.reports_dir,
        &machine,
        &collectors,
        &alerts,
        &incident_history,
    ) {
        tracing::warn!("report generation failed for {}: {e}", machine.hostname);
    }

    if let Err(e) = save_compliance_prompt_report(
        &state.reports_dir,
        &machine,
        &collectors,
        &alerts,
        &incident_history,
        admin.as_ref(),
        &state.company_profile,
        &state.freeze_prone_apps,
    ) {
        tracing::warn!("IA prompt report failed for {}: {e}", machine.hostname);
    }

    Ok(Json(InventoryResponse {
        accepted: collectors.len(),
        changed_collectors: changed.len(),
        health_score: score,
    }))
}

async fn list_machines(
    State(state): State<AppState>,
) -> Result<Json<Vec<MachineSummary>>, ApiError> {
    Ok(Json(state.db.list_machines()?))
}

async fn dashboard(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<MachineHighlight>>, ApiError> {
    if !auth::is_ti_authenticated(&state.db, &headers) {
        return Err(ApiError::Unauthorized);
    }
    let machines = state.db.list_machines()?;
    let mut out = Vec::with_capacity(machines.len());
    for m in machines {
        let collectors = state.db.get_machine_collectors(&m.id)?;
        let admin = state.db.get_machine_admin(&m.id)?;
        out.push(extract_highlights(
            m,
            &collectors,
            &admin,
            &state.company_profile,
        ));
    }
    Ok(Json(out))
}

async fn get_machine(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<MachineDetail>, ApiError> {
    if !auth::is_ti_authenticated(&state.db, &headers) {
        return Err(ApiError::Unauthorized);
    }
    let machine = state.db.get_machine_by_id(&id)?;
    let collectors = state.db.get_machine_collectors(&id)?;
    let admin = state.db.get_machine_admin(&id)?;
    let highlight =
        extract_highlights(machine.clone(), &collectors, &admin, &state.company_profile);
    let score_breakdown = compute_score_breakdown(&machine, &collectors);
    Ok(Json(MachineDetail {
        machine,
        collectors,
        highlight,
        admin,
        score_breakdown,
    }))
}

async fn update_machine_admin(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<UpdateMachineAdmin>,
) -> Result<Json<MachineAdmin>, ApiError> {
    if !auth::is_ti_authenticated(&state.db, &headers) {
        return Err(ApiError::Unauthorized);
    }
    let receive_departments_was_provided = payload.ticket_receive_departments.is_some();
    let mut record = payload.into_record();
    let mut receive_departments = Vec::new();
    for department in &record.ticket_receive_departments {
        let normalized = ticket_department_slug(department)
            .ok_or_else(|| ApiError::Internal("setor de recebimento invalido".into()))?;
        if !state.db.ticket_department_is_active(&normalized)? {
            return Err(ApiError::Internal(
                "setor de recebimento inexistente ou arquivado".into(),
            ));
        }
        if !receive_departments
            .iter()
            .any(|value: &String| value == &normalized)
        {
            receive_departments.push(normalized);
        }
    }
    record.ticket_receive_departments = receive_departments;
    let existing = state.db.get_machine_admin(&id)?;
    if !receive_departments_was_provided {
        record.ticket_receive_departments = existing.ticket_receive_departments.clone();
    }
    let admin = state.db.upsert_machine_admin(&id, &record)?;
    Ok(Json(admin))
}

async fn get_machine_tickets(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Vec<Ticket>>, ApiError> {
    if !auth::is_ti_authenticated(&state.db, &headers) {
        return Err(ApiError::Unauthorized);
    }
    let _ = state.db.get_machine_by_id(&id)?;
    Ok(Json(state.db.list_tickets(None, Some(&id))?))
}

async fn get_report(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let machine = state.db.get_machine_by_id(&id)?;
    let collectors = state.db.get_machine_collectors(&id)?;
    let alerts = state.db.list_alerts(true)?;
    let machine_alerts: Vec<_> = alerts.into_iter().filter(|a| a.machine_id == id).collect();
    let incidents = state.db.list_machine_incidents(&id, 80).unwrap_or_default();
    let content = generate_machine_report(
        &state.reports_dir,
        &machine,
        &collectors,
        &machine_alerts,
        &incidents,
    )
    .map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(content)
}

async fn get_compliance_prompt(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth::is_ti_authenticated(&state.db, &headers) {
        return Err(ApiError::Unauthorized);
    }
    let machine = state.db.get_machine_by_id(&id)?;
    let collectors = state.db.get_machine_collectors(&id)?;
    let alerts = state.db.list_alerts(true)?;
    let machine_alerts: Vec<_> = alerts.into_iter().filter(|a| a.machine_id == id).collect();
    let incidents = state.db.list_machine_incidents(&id, 80).unwrap_or_default();
    let admin = state.db.get_machine_admin(&id).ok();
    let md = generate_compliance_prompt_md(
        &machine,
        &collectors,
        &machine_alerts,
        &incidents,
        admin.as_ref(),
        &state.company_profile,
        &state.freeze_prone_apps,
    );
    Ok((
        [(
            axum::http::header::CONTENT_TYPE,
            "text/markdown; charset=utf-8",
        )],
        md,
    ))
}

fn save_compliance_prompt_report(
    reports_dir: &std::path::Path,
    machine: &MachineSummary,
    collectors: &[belarc_shared::CollectorResult],
    alerts: &[AlertRecord],
    incidents: &[IncidentRecord],
    admin: Option<&MachineAdmin>,
    profile: &belarc_shared::CompanyProfile,
    freeze_apps: &[crate::freeze_prone_apps::FreezeProneApp],
) -> Result<String, std::io::Error> {
    let md = generate_compliance_prompt_md(
        machine,
        collectors,
        alerts,
        incidents,
        admin,
        profile,
        freeze_apps,
    );
    let path = reports_dir.join(format!("{}.ia-prompt.md", machine.hostname));
    std::fs::write(&path, &md)?;
    Ok(md)
}

async fn get_machine_incidents(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<IncidentRecord>>, ApiError> {
    let _ = state.db.get_machine_by_id(&id)?;
    Ok(Json(state.db.list_machine_incidents(&id, 100)?))
}

async fn list_incidents(
    State(state): State<AppState>,
) -> Result<Json<Vec<IncidentRecord>>, ApiError> {
    Ok(Json(state.db.list_all_incidents(200)?))
}

async fn maintenance_playbook(
    State(state): State<AppState>,
) -> Json<crate::maintenance::MaintenancePlaybook> {
    Json(crate::maintenance::maintenance_playbook(
        &state.company_profile,
        &state.freeze_prone_apps,
    ))
}

#[derive(Serialize)]
struct FleetHealthBandCounts {
    excellent: u32,
    good: u32,
    attention: u32,
    problem: u32,
    critical: u32,
}

#[derive(Serialize)]
struct FleetMachineHealth {
    id: String,
    hostname: String,
    score: Option<u8>,
    band: String,
    alert_count: u32,
    status: String,
}

#[derive(Serialize)]
struct FleetHealthResponse {
    avg_score: Option<u8>,
    bands: FleetHealthBandCounts,
    machines: Vec<FleetMachineHealth>,
}

async fn fleet_health(
    State(state): State<AppState>,
) -> Result<Json<FleetHealthResponse>, ApiError> {
    let machines = state.db.list_machines()?;
    let alerts = state.db.list_alerts(true)?;

    let mut bands = FleetHealthBandCounts {
        excellent: 0,
        good: 0,
        attention: 0,
        problem: 0,
        critical: 0,
    };
    let mut score_sum: u64 = 0;
    let mut score_count: u64 = 0;

    let fleet_machines: Vec<FleetMachineHealth> = machines
        .iter()
        .map(|m| {
            let score = m.health_score;
            let band = score.map(score_band).unwrap_or("—").to_string();
            if let Some(s) = score {
                score_sum += s as u64;
                score_count += 1;
                match s {
                    90..=100 => bands.excellent += 1,
                    75..=89 => bands.good += 1,
                    50..=74 => bands.attention += 1,
                    25..=49 => bands.problem += 1,
                    _ => bands.critical += 1,
                }
            }
            let alert_count = alerts.iter().filter(|a| a.machine_id == m.id).count() as u32;
            FleetMachineHealth {
                id: m.id.clone(),
                hostname: m.hostname.clone(),
                score,
                band,
                alert_count,
                status: format!("{:?}", m.status).to_lowercase(),
            }
        })
        .collect();

    let avg_score = if score_count > 0 {
        Some((score_sum / score_count) as u8)
    } else {
        None
    };

    Ok(Json(FleetHealthResponse {
        avg_score,
        bands,
        machines: fleet_machines,
    }))
}

#[derive(Deserialize)]
struct AlertsQuery {
    unresolved: Option<bool>,
}

async fn list_alerts(
    State(state): State<AppState>,
    Query(q): Query<AlertsQuery>,
) -> Result<Json<Vec<belarc_shared::AlertRecord>>, ApiError> {
    Ok(Json(state.db.list_alerts(q.unresolved.unwrap_or(true))?))
}

async fn resolve_alert(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<belarc_shared::AlertRecord>, ApiError> {
    if !auth::is_ti_authenticated(&state.db, &headers) {
        return Err(ApiError::Unauthorized);
    }
    Ok(Json(state.db.resolve_alert(&id)?))
}

#[derive(Serialize)]
struct AlertTicketResponse {
    ticket: Ticket,
    alert: belarc_shared::AlertRecord,
}

async fn create_ticket_from_alert(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<AlertTicketResponse>), ApiError> {
    if !auth::is_ti_authenticated(&state.db, &headers) {
        return Err(ApiError::Unauthorized);
    }
    let alert = state.db.get_alert_by_id(&id)?;
    let title = {
        let msg = alert.message.trim();
        let truncated = if msg.chars().count() > 80 {
            format!("{}…", msg.chars().take(77).collect::<String>())
        } else {
            msg.to_string()
        };
        format!("[{}] {}", alert.category, truncated)
    };
    let description = format!(
        "Chamado gerado a partir de alerta.\n\n\
         Host: {}\n\
         Severidade: {:?}\n\
         Categoria: {}\n\
         Mensagem: {}\n\
         Alerta ID: {}\n",
        alert.hostname, alert.severity, alert.category, alert.message, alert.id
    );
    let priority = match alert.severity {
        belarc_shared::AlertSeverity::Critical => "high",
        belarc_shared::AlertSeverity::Warning => "normal",
        belarc_shared::AlertSeverity::Info => "low",
    };
    let machine = state.db.get_machine_by_id(&alert.machine_id)?;
    let owner_snap = state
        .db
        .get_machine_admin(&alert.machine_id)
        .ok()
        .and_then(|a| a.owner_name)
        .filter(|s| !s.trim().is_empty())
        .or_else(|| machine.logged_user.clone().filter(|s| !s.trim().is_empty()));
    let mut ticket = state.db.create_ticket(
        &alert.machine_id,
        Some(&machine.hostname),
        owner_snap.as_deref(),
        &title,
        Some(&description),
        priority,
        "ti",
        "ti",
        None,
    )?;
    mirror_ticket_to_nas(&state, &mut ticket);
    let alert = if !alert.resolved {
        state.db.resolve_alert(&id).unwrap_or(alert)
    } else {
        alert
    };
    let ticket = state.db.get_ticket_by_id(&ticket.id)?;
    Ok((
        StatusCode::CREATED,
        Json(AlertTicketResponse { ticket, alert }),
    ))
}

async fn get_config(State(state): State<AppState>) -> Json<ServerConfig> {
    Json(state.config.clone())
}

async fn get_company_profile(State(state): State<AppState>) -> Json<belarc_shared::CompanyProfile> {
    Json(state.company_profile.clone())
}

#[derive(Deserialize)]
struct CreateTokenRequest {
    hostname: Option<String>,
}

async fn create_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateTokenRequest>,
) -> Result<Json<CreateTokenResponse>, ApiError> {
    // Tokens de agente permitem registrar e atualizar inventário: nunca podem
    // ser emitidos por uma chamada anônima, mesmo em uma LAN privada.
    if !auth::is_ti_authenticated(&state.db, &headers) {
        return Err(ApiError::Unauthorized);
    }
    let token = state.db.create_agent_token(req.hostname.as_deref())?;
    Ok(Json(CreateTokenResponse { token }))
}

async fn admin_reset(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !auth::is_ti_authenticated(&state.db, &headers) {
        return Err(ApiError::Unauthorized);
    }
    state.db.reset_all_data()?;
    let reports = std::fs::read_dir(&state.reports_dir).ok();
    if let Some(entries) = reports {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    tracing::warn!("database and reports reset via /api/admin/reset");
    Ok(Json(
        serde_json::json!({ "status": "reset", "message": "Todos os dados foram apagados" }),
    ))
}

async fn auth_login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, ApiError> {
    let username = req.username.trim();
    if username.is_empty() || req.password.is_empty() {
        return Err(ApiError::Unauthorized);
    }
    let Some((user_id, uname, hash)) = state
        .db
        .get_ti_user_by_username(username)
        .map_err(ApiError::from)?
    else {
        return Err(ApiError::Unauthorized);
    };
    if !auth::verify_password(&req.password, &hash) {
        return Err(ApiError::Unauthorized);
    }
    let (token, expires_at) = state
        .db
        .create_ti_session(&user_id, SESSION_HOURS)
        .map_err(ApiError::from)?;
    Ok(Json(LoginResponse {
        token,
        username: uname,
        expires_at,
    }))
}

async fn auth_logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    if let Some(token) = auth::bearer_token(&headers) {
        let _ = state.db.delete_ti_session(&token);
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn auth_me(State(state): State<AppState>, headers: HeaderMap) -> Json<MeResponse> {
    match auth::session_username(&state.db, &headers) {
        Some(username) => Json(MeResponse {
            authenticated: true,
            username: Some(username),
        }),
        None => Json(MeResponse {
            authenticated: false,
            username: None,
        }),
    }
}

enum TicketCaller {
    Ti,
    Agent { machine_id: String },
    Portal(crate::db::PortalUser),
}

#[derive(Deserialize)]
struct TicketDepartmentRequest {
    name: String,
}

fn ticket_department_slug(value: &str) -> Option<String> {
    if let Some(slug) = tickets::normalize_department(value) {
        return Some(slug.to_string());
    }
    let slug = value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>();
    let slug = slug
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    (!slug.is_empty() && slug.len() <= 48).then_some(slug)
}

fn ticket_department_name(value: &str) -> Result<String, ApiError> {
    let name = value.trim();
    if name.is_empty() || name.chars().count() > 60 {
        return Err(ApiError::Internal(
            "nome do setor deve ter entre 1 e 60 caracteres".into(),
        ));
    }
    Ok(name.to_string())
}

async fn list_ticket_departments(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Vec<crate::db::TicketDepartment>>, ApiError> {
    let include_inactive = query.get("include_inactive").map(String::as_str) == Some("true");
    if include_inactive && !auth::is_ti_authenticated(&state.db, &headers) {
        return Err(ApiError::Unauthorized);
    }
    Ok(Json(state.db.list_ticket_departments(include_inactive)?))
}

async fn create_ticket_department(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<TicketDepartmentRequest>,
) -> Result<(StatusCode, Json<crate::db::TicketDepartment>), ApiError> {
    if !auth::is_ti_authenticated(&state.db, &headers) {
        return Err(ApiError::Unauthorized);
    }
    let name = ticket_department_name(&req.name)?;
    let slug = ticket_department_slug(&name)
        .ok_or_else(|| ApiError::Internal("nome do setor invalido".into()))?;
    if state.db.ticket_department_exists(&slug)? {
        return Err(ApiError::Conflict(
            "ja existe um setor com este identificador".into(),
        ));
    }
    Ok((
        StatusCode::CREATED,
        Json(state.db.create_ticket_department(&slug, &name)?),
    ))
}

async fn rename_ticket_department(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(req): Json<TicketDepartmentRequest>,
) -> Result<Json<crate::db::TicketDepartment>, ApiError> {
    if !auth::is_ti_authenticated(&state.db, &headers) {
        return Err(ApiError::Unauthorized);
    }
    let slug =
        ticket_department_slug(&slug).ok_or_else(|| ApiError::Internal("setor invalido".into()))?;
    Ok(Json(state.db.rename_ticket_department(
        &slug,
        &ticket_department_name(&req.name)?,
    )?))
}

async fn archive_ticket_department(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<StatusCode, ApiError> {
    if !auth::is_ti_authenticated(&state.db, &headers) {
        return Err(ApiError::Unauthorized);
    }
    let slug =
        ticket_department_slug(&slug).ok_or_else(|| ApiError::Internal("setor invalido".into()))?;
    state
        .db
        .archive_ticket_department(&slug)
        .map_err(|err| match err {
            crate::db::DbError::InvalidInput(message) => ApiError::Conflict(message),
            other => ApiError::from(other),
        })?;
    Ok(StatusCode::NO_CONTENT)
}

/// Configuração independente do recebimento de chamados. Ela não passa pelo
/// formulário de Cadastro TI para não limpar campos administrativos quando a
/// TI só marca/desmarca setores na aba Chamados do PC.
#[derive(Deserialize)]
struct UpdateMachineTicketRouting {
    #[serde(default)]
    receive_departments: Vec<String>,
}

async fn update_machine_ticket_routing(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<UpdateMachineTicketRouting>,
) -> Result<Json<MachineAdmin>, ApiError> {
    if !auth::is_ti_authenticated(&state.db, &headers) {
        return Err(ApiError::Unauthorized);
    }
    let _ = state.db.get_machine_by_id(&id)?;
    let mut departments = Vec::new();
    for department in payload.receive_departments {
        let normalized = ticket_department_slug(&department)
            .ok_or_else(|| ApiError::Internal("setor de recebimento invalido".into()))?;
        if !state.db.ticket_department_is_active(&normalized)? {
            return Err(ApiError::Internal(
                "setor de recebimento inexistente ou arquivado".into(),
            ));
        }
        if !departments
            .iter()
            .any(|value: &String| value == &normalized)
        {
            departments.push(normalized);
        }
    }
    state.db.set_machine_ticket_departments(&id, &departments)?;
    Ok(Json(state.db.get_machine_admin(&id)?))
}

#[derive(Deserialize)]
struct PortalUserCreateRequest {
    username: String,
    display_name: String,
    password: String,
    role: String,
    department: Option<String>,
}
#[derive(Serialize)]
struct PortalLoginResponse {
    token: String,
    username: String,
    display_name: String,
    role: String,
    department: Option<String>,
    expires_at: String,
}
#[derive(Serialize)]
struct PortalMeResponse {
    authenticated: bool,
    username: Option<String>,
    display_name: Option<String>,
    role: Option<String>,
    department: Option<String>,
    can_open_ticket: bool,
    receive_departments: Vec<String>,
}

async fn portal_login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<PortalLoginResponse>, ApiError> {
    let user = state
        .db
        .get_portal_user_by_username(&req.username)?
        .ok_or(ApiError::Unauthorized)?;
    if !auth::verify_password(&req.password, &user.password_hash) {
        return Err(ApiError::Unauthorized);
    }
    let (token, expires_at) = state.db.create_portal_session(&user.id, SESSION_HOURS)?;
    Ok(Json(PortalLoginResponse {
        token,
        username: user.username,
        display_name: user.display_name,
        role: user.role,
        department: user.department,
        expires_at,
    }))
}
async fn portal_logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    if let Some(token) = auth::bearer_token(&headers) {
        state.db.delete_portal_session(&token)?;
    }
    Ok(StatusCode::NO_CONTENT)
}
async fn portal_me(State(state): State<AppState>, headers: HeaderMap) -> Json<PortalMeResponse> {
    let user = auth::bearer_token(&headers)
        .and_then(|t| state.db.validate_portal_session(&t).ok().flatten());
    Json(match user {
        Some(u) => {
            let mut receive_departments = state
                .db
                .list_portal_user_departments(&u.id)
                .unwrap_or_default();
            if receive_departments.is_empty() && u.role == "sector_agent" {
                receive_departments = u.department.clone().into_iter().collect();
            }
            let can_open_ticket = u.role == "requester" || u.password_hash == "device-session-only";
            PortalMeResponse {
                authenticated: true,
                username: Some(u.username),
                display_name: Some(u.display_name),
                role: Some(u.role),
                department: u.department,
                can_open_ticket,
                receive_departments,
            }
        }
        None => PortalMeResponse {
            authenticated: false,
            username: None,
            display_name: None,
            role: None,
            department: None,
            can_open_ticket: false,
            receive_departments: Vec::new(),
        },
    })
}
async fn portal_machines(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    let token = auth::bearer_token(&headers).ok_or(ApiError::Unauthorized)?;
    let user = state
        .db
        .validate_portal_session(&token)?
        .ok_or(ApiError::Unauthorized)?;
    tracing::info!(username = %user.username, role = %user.role, "portal machine list authorized");
    if user.role != "requester" && !is_device_portal_user(&user) {
        return Err(ApiError::Unauthorized);
    }
    Ok(Json(
        state
            .db
            .list_portal_user_machines(&user.id)?
            .into_iter()
            .map(|(id, hostname)| serde_json::json!({"id":id,"hostname":hostname}))
            .collect(),
    ))
}

#[derive(Deserialize)]
struct DevicePortalSessionRequest {
    #[serde(default)]
    mode: Option<String>,
}
#[derive(Serialize)]
struct DevicePortalSessionResponse {
    token: String,
    portal_path: String,
    receive_departments: Vec<String>,
    expires_at: String,
}
#[derive(Serialize)]
struct DevicePortalRoutingResponse {
    receive_departments: Vec<String>,
}

/// Rota exclusiva do agente. O navegador nunca recebe o token de agente: ele
/// recebe somente esta sessão limitada, que fica no fragmento da URL e é limpa
/// pelo cliente imediatamente após a abertura.
async fn create_device_portal_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<DevicePortalSessionRequest>,
) -> Result<Json<DevicePortalSessionResponse>, ApiError> {
    let agent_token = auth::bearer_token(&headers).ok_or(ApiError::Unauthorized)?;
    let machine_id = state.db.machine_id_for_agent_token(&agent_token)?;
    let machine = state.db.get_machine_by_id(&machine_id)?;
    let admin = state.db.get_machine_admin(&machine_id)?;
    let _legacy_mode = req.mode.as_deref(); // requesters/attendants antigos agora abrem o mesmo portal unificado.
    let receive_departments = admin
        .ticket_receive_departments
        .into_iter()
        .filter(|d| state.db.ticket_department_is_active(d).unwrap_or(false))
        .collect::<Vec<_>>();
    let display_name = admin
        .owner_name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .map(str::to_string)
        .or(machine.logged_user.clone())
        .unwrap_or_else(|| format!("Portal — {}", machine.hostname));
    let user =
        state
            .db
            .ensure_device_portal_user(&machine_id, &receive_departments, &display_name)?;
    let (token, expires_at) = state.db.create_portal_session(&user.id, 8)?;
    Ok(Json(DevicePortalSessionResponse {
        token,
        portal_path: "/cliente.html".into(),
        receive_departments,
        expires_at,
    }))
}

/// Consulta exclusiva do agente instalado. Ela sincroniza a configuração feita
/// pela TI para que um cliente já aberto passe a receber (ou deixe de receber)
/// setores sem login, reinicialização ou criação repetida de sessões.
async fn device_portal_routing(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<DevicePortalRoutingResponse>, ApiError> {
    let agent_token = auth::bearer_token(&headers).ok_or(ApiError::Unauthorized)?;
    let machine_id = state.db.machine_id_for_agent_token(&agent_token)?;
    let machine = state.db.get_machine_by_id(&machine_id)?;
    let admin = state.db.get_machine_admin(&machine_id)?;
    let receive_departments = admin
        .ticket_receive_departments
        .into_iter()
        .filter(|department| {
            state
                .db
                .ticket_department_is_active(department)
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    let display_name = admin
        .owner_name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .map(str::to_string)
        .or(machine.logged_user.clone())
        .unwrap_or_else(|| format!("Portal — {}", machine.hostname));
    // Mantém a mesma conta técnica e atualiza suas permissões; a sessão curta
    // já existente continua válida porque ela referencia essa conta.
    state
        .db
        .ensure_device_portal_user(&machine_id, &receive_departments, &display_name)?;
    Ok(Json(DevicePortalRoutingResponse {
        receive_departments,
    }))
}
async fn create_portal_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PortalUserCreateRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    if !auth::is_ti_authenticated(&state.db, &headers) {
        return Err(ApiError::Unauthorized);
    }
    let role = req.role.trim();
    let department = req.department.as_deref().and_then(ticket_department_slug);
    if req.department.is_some()
        && (department.is_none()
            || !state
                .db
                .ticket_department_is_active(department.as_deref().unwrap_or_default())?)
    {
        return Err(ApiError::Internal("setor invalido ou arquivado".into()));
    }
    if req.password.len() < 8 {
        return Err(ApiError::Internal(
            "senha deve ter ao menos 8 caracteres".into(),
        ));
    }
    let hash = auth::hash_password(&req.password).map_err(ApiError::Internal)?;
    let user = state.db.create_portal_user(
        &req.username,
        &req.display_name,
        &hash,
        role,
        department.as_deref(),
    )?;
    if let Some(department) = user.department.clone() {
        state
            .db
            .set_portal_user_departments(&user.id, &[department])?;
    }
    Ok((
        StatusCode::CREATED,
        Json(
            serde_json::json!({"id":user.id,"username":user.username,"display_name":user.display_name,"role":user.role,"department":user.department}),
        ),
    ))
}
async fn link_portal_user_machine(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, machine_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    if !auth::is_ti_authenticated(&state.db, &headers) {
        return Err(ApiError::Unauthorized);
    }
    state.db.link_portal_user_machine(&id, &machine_id)?;
    Ok(StatusCode::NO_CONTENT)
}

fn resolve_ticket_caller(
    db: &crate::db::Database,
    headers: &HeaderMap,
) -> Result<TicketCaller, ApiError> {
    if auth::is_ti_authenticated(db, headers) {
        return Ok(TicketCaller::Ti);
    }
    if let Some(token) = auth::bearer_token(headers) {
        if let Some(user) = db.validate_portal_session(&token)? {
            return Ok(TicketCaller::Portal(user));
        }
        let machine_id = db.machine_id_for_agent_token(&token)?;
        return Ok(TicketCaller::Agent { machine_id });
    }
    Err(ApiError::Unauthorized)
}

fn portal_receive_departments(
    db: &crate::db::Database,
    user: &crate::db::PortalUser,
) -> Result<Vec<String>, ApiError> {
    let mut departments = db.list_portal_user_departments(&user.id)?;
    if departments.is_empty() && user.role == "sector_agent" {
        departments.extend(user.department.clone());
    }
    Ok(departments)
}
fn is_device_portal_user(user: &crate::db::PortalUser) -> bool {
    user.password_hash == "device-session-only"
}
fn portal_can_read_ticket(
    db: &crate::db::Database,
    user: &crate::db::PortalUser,
    ticket: &Ticket,
) -> Result<bool, ApiError> {
    Ok(
        ticket.requester_user_id.as_deref() == Some(user.id.as_str())
            || portal_receive_departments(db, user)?
                .iter()
                .any(|department| department == &ticket.department),
    )
}
fn portal_can_manage_ticket(
    db: &crate::db::Database,
    user: &crate::db::PortalUser,
    ticket: &Ticket,
) -> Result<bool, ApiError> {
    Ok(portal_receive_departments(db, user)?
        .iter()
        .any(|department| department == &ticket.department))
}

#[derive(Deserialize)]
struct TicketListQuery {
    status: Option<String>,
    department: Option<String>,
}

async fn create_ticket(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateTicketRequest>,
) -> Result<(StatusCode, Json<Ticket>), ApiError> {
    let title = req.title.trim();
    if title.is_empty() {
        return Err(ApiError::Internal("title required".into()));
    }
    let priority = req
        .priority
        .as_deref()
        .and_then(tickets::normalize_priority)
        .unwrap_or("normal");

    let (machine_id, created_by, requester_user_id) =
        match resolve_ticket_caller(&state.db, &headers)? {
            TicketCaller::Agent { machine_id } => (machine_id, "agent", None),
            TicketCaller::Ti => {
                let mid = req
                    .machine_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| ApiError::Internal("machine_id required for TI".into()))?
                    .to_string();
                (mid, "ti", None)
            }
            TicketCaller::Portal(user) => {
                if user.role != "requester" && !is_device_portal_user(&user) {
                    return Err(ApiError::Unauthorized);
                }
                let mid = req
                    .machine_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| ApiError::Internal("selecione o computador".into()))?
                    .to_string();
                if !state.db.portal_user_has_machine(&user.id, &mid)? {
                    return Err(ApiError::Unauthorized);
                }
                (mid, "portal", Some(user.id))
            }
        };

    let machine = state.db.get_machine_by_id(&machine_id)?;
    let owner_snap = state
        .db
        .get_machine_admin(&machine_id)
        .ok()
        .and_then(|a| a.owner_name)
        .filter(|s| !s.trim().is_empty())
        .or_else(|| machine.logged_user.clone().filter(|s| !s.trim().is_empty()));
    let department = match req.department.as_deref() {
        Some(value) => ticket_department_slug(value)
            .ok_or_else(|| ApiError::Internal("setor invalido".into()))?,
        None => "ti".to_string(),
    };
    if !state.db.ticket_department_is_active(&department)? {
        return Err(ApiError::Internal("setor inexistente ou arquivado".into()));
    }
    let mut ticket = state.db.create_ticket(
        &machine_id,
        Some(&machine.hostname),
        owner_snap.as_deref(),
        title,
        req.description.as_deref(),
        priority,
        created_by,
        &department,
        requester_user_id.as_deref(),
    )?;

    mirror_ticket_to_nas(&state, &mut ticket);

    // Anexos: SYSTEM muitas vezes nao escreve no NAS. Nao falhar o create —
    // Belarc Chamado grava os bytes no share; sync/attach registra metadados.
    for att in &req.attachments {
        if let Err(e) = store_ticket_attachment(&state, &ticket.id, att).await {
            tracing::warn!("attachment on create {} (soft): {:?}", ticket.code, e);
        }
    }

    let mut ticket = state.db.get_ticket_by_id(&ticket.id)?;
    mirror_ticket_to_nas(&state, &mut ticket);
    Ok((StatusCode::CREATED, Json(ticket)))
}

async fn store_ticket_attachment(
    state: &AppState,
    ticket_id: &str,
    req: &AttachRequest,
) -> Result<(), ApiError> {
    let ticket = state.db.get_ticket_by_id(ticket_id)?;
    let unc_dir = tickets::prefer_unc_path(&state.chamados_root, &ticket);
    let _ = state
        .db
        .set_ticket_nas_path(&ticket.id, &unc_dir.to_string_lossy());

    let filename = req.filename.trim();
    if filename.is_empty() {
        return Err(ApiError::Internal("filename required".into()));
    }
    let safe = req
        .stored_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(tickets::sanitize_filename)
        .unwrap_or_else(|| tickets::sanitize_filename(filename));

    // Register-only: Belarc Chamado ja gravou bytes no NAS.
    if req.content_base64.trim().is_empty() {
        let existing = state.db.list_ticket_attachments(&ticket.id)?;
        if existing
            .iter()
            .any(|a| a.stored_name == safe || a.filename == filename)
        {
            return Ok(());
        }
        let _ = state
            .db
            .add_ticket_attachment(&ticket.id, filename, &safe)?;
        if let Ok(mut t) = state.db.get_ticket_by_id(ticket_id) {
            let unc = tickets::unc_ticket_dir(&state.chamados_root, &t.code)
                .to_string_lossy()
                .to_string();
            let _ = state.db.set_ticket_nas_path(&t.id, &unc);
            t.nas_path = Some(unc);
            mirror_ticket_to_nas(state, &mut t);
        }
        return Ok(());
    }

    let bytes = tickets::decode_base64(&req.content_base64)
        .map_err(|e| ApiError::Internal(format!("base64: {e}")))?;
    if bytes.is_empty() {
        return Err(ApiError::Internal("empty attachment".into()));
    }
    if bytes.len() > 512 * 1024 * 1024 {
        return Err(ApiError::Internal(
            "attachment too large (max 512MB)".into(),
        ));
    }

    let dir = match tickets::provision_nas_folder(&state.chamados_root, &ticket) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("NAS provision attach {}: {e}", ticket.code);
            unc_dir.clone()
        }
    };

    let stored = match tickets::write_attachment_file(&dir, filename, &bytes) {
        Ok(s) => s,
        Err(e) => {
            let att_dir = dir.join("attachments");
            if att_dir.is_dir() {
                if let Ok(rd) = std::fs::read_dir(&att_dir) {
                    for ent in rd.flatten() {
                        let name = ent.file_name().to_string_lossy().to_string();
                        if name.ends_with(&safe) || name == safe {
                            let existing = state.db.list_ticket_attachments(&ticket.id)?;
                            if !existing.iter().any(|a| a.stored_name == name) {
                                let _ = state
                                    .db
                                    .add_ticket_attachment(&ticket.id, filename, &name)?;
                            }
                            if let Ok(mut t) = state.db.get_ticket_by_id(ticket_id) {
                                let unc = tickets::unc_ticket_dir(&state.chamados_root, &t.code)
                                    .to_string_lossy()
                                    .to_string();
                                let _ = state.db.set_ticket_nas_path(&t.id, &unc);
                                t.nas_path = Some(unc);
                                mirror_ticket_to_nas(state, &mut t);
                            }
                            return Ok(());
                        }
                    }
                }
            }
            tracing::warn!(
                "NAS write attach {} falhou ({}); registrando metadado ({})",
                ticket.code,
                e,
                safe
            );
            let existing = state.db.list_ticket_attachments(&ticket.id)?;
            if !existing.iter().any(|a| a.stored_name == safe) {
                let _ = state
                    .db
                    .add_ticket_attachment(&ticket.id, filename, &safe)?;
            }
            if let Ok(mut t) = state.db.get_ticket_by_id(ticket_id) {
                let unc = tickets::unc_ticket_dir(&state.chamados_root, &t.code)
                    .to_string_lossy()
                    .to_string();
                let _ = state.db.set_ticket_nas_path(&t.id, &unc);
                t.nas_path = Some(unc);
                mirror_ticket_to_nas(state, &mut t);
            }
            return Ok(());
        }
    };
    let existing = state.db.list_ticket_attachments(&ticket.id)?;
    if !existing.iter().any(|a| a.stored_name == stored) {
        let _ = state
            .db
            .add_ticket_attachment(&ticket.id, filename, &stored)?;
    }
    if let Ok(mut t) = state.db.get_ticket_by_id(ticket_id) {
        let unc = tickets::unc_ticket_dir(&state.chamados_root, &t.code)
            .to_string_lossy()
            .to_string();
        let _ = state.db.set_ticket_nas_path(&t.id, &unc);
        t.nas_path = Some(unc);
        mirror_ticket_to_nas(state, &mut t);
    }
    Ok(())
}

fn mirror_ticket_to_nas(state: &AppState, ticket: &mut Ticket) {
    match tickets::provision_ticket_folder(&state.chamados_root, ticket) {
        Ok(path) => {
            let unc = tickets::unc_ticket_dir(&state.chamados_root, &ticket.code);
            let path_str = if path
                .to_string_lossy()
                .to_ascii_lowercase()
                .contains("chamados-files")
            {
                unc.to_string_lossy().to_string()
            } else {
                path.to_string_lossy().to_string()
            };
            if let Err(e) = state.db.set_ticket_nas_path(&ticket.id, &path_str) {
                tracing::warn!("ticket {} nas_path update failed: {e}", ticket.code);
            } else {
                ticket.nas_path = Some(path_str);
            }
        }
        Err(e) => tracing::warn!("ticket {} NAS provision: {e}", ticket.code),
    }
    // Best-effort export of index/xlsx
    if let Ok(list) = state.db.list_tickets(None, None) {
        if let Err(e) = tickets::sync_chamados_export(&state.chamados_root, &list) {
            tracing::debug!("chamados export: {e}");
        }
    }
}

#[derive(Serialize)]
struct TicketStatsByStatus {
    open: u32,
    in_progress: u32,
    waiting: u32,
    done: u32,
}

#[derive(Serialize)]
struct TicketAgeBuckets {
    under_1d: u32,
    d1_to_7: u32,
    over_7d: u32,
}

#[derive(Serialize)]
struct TicketStatsResponse {
    total: u32,
    by_status: TicketStatsByStatus,
    open_age: TicketAgeBuckets,
    rated_count: u32,
    rating_avg: Option<f64>,
}

fn ticket_open_age_days(created_at: &str) -> Option<i64> {
    let dt = chrono::DateTime::parse_from_rfc3339(created_at)
        .ok()?
        .with_timezone(&chrono::Utc);
    let secs = (chrono::Utc::now() - dt).num_seconds();
    Some(secs / 86400)
}

async fn ticket_stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<TicketStatsResponse>, ApiError> {
    if !auth::is_ti_authenticated(&state.db, &headers) {
        return Err(ApiError::Unauthorized);
    }
    let list = state.db.list_tickets(None, None)?;
    let mut by_status = TicketStatsByStatus {
        open: 0,
        in_progress: 0,
        waiting: 0,
        done: 0,
    };
    let mut open_age = TicketAgeBuckets {
        under_1d: 0,
        d1_to_7: 0,
        over_7d: 0,
    };
    let mut rating_sum: i64 = 0;
    let mut rated_count: u32 = 0;
    for t in &list {
        match t.status.as_str() {
            "in_progress" => by_status.in_progress += 1,
            "waiting" => by_status.waiting += 1,
            "done" => by_status.done += 1,
            _ => by_status.open += 1,
        }
        if t.status != "done" {
            match ticket_open_age_days(&t.created_at) {
                Some(d) if d < 1 => open_age.under_1d += 1,
                Some(d) if d <= 7 => open_age.d1_to_7 += 1,
                Some(_) => open_age.over_7d += 1,
                None => open_age.under_1d += 1,
            }
        }
        if let Some(r) = t.rating {
            rated_count += 1;
            rating_sum += r as i64;
        }
    }
    let rating_avg = if rated_count > 0 {
        Some(rating_sum as f64 / rated_count as f64)
    } else {
        None
    };
    Ok(Json(TicketStatsResponse {
        total: list.len() as u32,
        by_status,
        open_age,
        rated_count,
        rating_avg,
    }))
}

async fn list_tickets(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<TicketListQuery>,
) -> Result<Json<Vec<Ticket>>, ApiError> {
    let status = q
        .status
        .as_deref()
        .and_then(tickets::normalize_status)
        .map(|s| s.to_string());
    let caller = resolve_ticket_caller(&state.db, &headers)?;
    let department = q.department.as_deref().and_then(ticket_department_slug);
    let list = match caller {
        TicketCaller::Ti => {
            let list = state.db.list_tickets(status.as_deref(), None)?;
            match department {
                Some(ref department) => list
                    .into_iter()
                    .filter(|ticket| ticket.department == *department)
                    .collect(),
                None => list,
            }
        }
        TicketCaller::Portal(user) => {
            let departments = portal_receive_departments(&state.db, &user)?;
            if departments.is_empty() {
                return Ok(Json(state.db.list_tickets_for_requester(&user.id)?));
            }
            let mut list = state.db.list_tickets_for_departments(&departments)?;
            if let Some(status) = status {
                list.retain(|ticket| ticket.status == status);
            }
            list
        }
        _ => return Err(ApiError::Unauthorized),
    };
    Ok(Json(list))
}

async fn list_my_tickets(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Ticket>>, ApiError> {
    let machine_id = match resolve_ticket_caller(&state.db, &headers)? {
        TicketCaller::Agent { machine_id } => machine_id,
        TicketCaller::Ti => {
            return Err(ApiError::Internal(
                "use GET /api/tickets for TI listing".into(),
            ));
        }
        TicketCaller::Portal(user) => {
            return Ok(Json(state.db.list_tickets_for_requester(&user.id)?))
        }
    };
    let list = state.db.list_tickets(None, Some(&machine_id))?;
    Ok(Json(list))
}

async fn get_ticket(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Ticket>, ApiError> {
    let ticket = state
        .db
        .get_ticket_by_id(&id)
        .or_else(|_| state.db.get_ticket_by_code(&id))?;
    match resolve_ticket_caller(&state.db, &headers)? {
        TicketCaller::Ti => Ok(Json(ticket)),
        TicketCaller::Agent { machine_id } => {
            if ticket.machine_id == machine_id {
                Ok(Json(ticket))
            } else {
                Err(ApiError::Unauthorized)
            }
        }
        TicketCaller::Portal(user) => {
            if portal_can_read_ticket(&state.db, &user, &ticket)? {
                Ok(Json(ticket))
            } else {
                Err(ApiError::Unauthorized)
            }
        }
    }
}

async fn delete_ticket(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    if !auth::is_ti_authenticated(&state.db, &headers) {
        return Err(ApiError::Unauthorized);
    }
    let ticket = state
        .db
        .get_ticket_by_id(&id)
        .or_else(|_| state.db.get_ticket_by_code(&id))?;
    tickets::remove_ticket_folder(&state.chamados_root, &ticket);
    state.db.delete_ticket(&ticket.id)?;
    if let Ok(list) = state.db.list_tickets(None, None) {
        if let Err(e) = tickets::sync_chamados_export(&state.chamados_root, &list) {
            tracing::warn!("chamados export apos delete {}: {e}", ticket.code);
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn update_ticket(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<UpdateTicketRequest>,
) -> Result<Json<Ticket>, ApiError> {
    let existing = state
        .db
        .get_ticket_by_id(&id)
        .or_else(|_| state.db.get_ticket_by_code(&id))?;
    let caller = resolve_ticket_caller(&state.db, &headers)?;
    let ti_caller = matches!(caller, TicketCaller::Ti);
    match &caller {
        TicketCaller::Ti => {}
        TicketCaller::Portal(user) if portal_can_manage_ticket(&state.db, user, &existing)? => {}
        _ => return Err(ApiError::Unauthorized),
    }
    if existing.status == tickets::STATUS_DONE {
        return Err(ApiError::Conflict(
            "chamado concluido — alteracoes bloqueadas".into(),
        ));
    }
    let status = match req.status.as_deref() {
        Some(s) => Some(
            tickets::normalize_status(s)
                .ok_or_else(|| ApiError::Internal("invalid status".into()))?,
        ),
        None => None,
    };
    let priority = match req.priority.as_deref() {
        Some(p) => Some(
            tickets::normalize_priority(p)
                .ok_or_else(|| ApiError::Internal("invalid priority".into()))?,
        ),
        None => None,
    };
    // Valida a transferencia antes de qualquer escrita: um atendente de setor
    // nao pode provocar uma atualizacao parcial enviando department no PATCH.
    let department_update = match req.department.as_deref() {
        Some(value) if ti_caller => {
            let department = ticket_department_slug(value)
                .ok_or_else(|| ApiError::Internal("setor invalido".into()))?;
            if !state.db.ticket_department_is_active(&department)? {
                return Err(ApiError::Internal("setor inexistente ou arquivado".into()));
            }
            Some(department)
        }
        Some(_) => return Err(ApiError::Unauthorized),
        None => None,
    };
    let mut ticket = state.db.update_ticket(
        &existing.id,
        req.title.as_deref(),
        req.description.as_deref(),
        status,
        priority,
        req.assignee.as_deref(),
    )?;
    if let Some(department) = department_update {
        ticket = state.db.set_ticket_department(&ticket.id, &department)?;
    }
    mirror_ticket_to_nas(&state, &mut ticket);
    Ok(Json(ticket))
}

async fn attach_ticket_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<AttachRequest>,
) -> Result<Json<Ticket>, ApiError> {
    let ticket = state
        .db
        .get_ticket_by_id(&id)
        .or_else(|_| state.db.get_ticket_by_code(&id))?;
    if ticket.status == tickets::STATUS_DONE {
        return Err(ApiError::Conflict(
            "chamado concluido — anexos bloqueados".into(),
        ));
    }
    match resolve_ticket_caller(&state.db, &headers)? {
        TicketCaller::Ti => {}
        TicketCaller::Agent { machine_id } => {
            if ticket.machine_id != machine_id {
                return Err(ApiError::Unauthorized);
            }
        }
        TicketCaller::Portal(user) => {
            if !portal_can_read_ticket(&state.db, &user, &ticket)? {
                return Err(ApiError::Unauthorized);
            }
        }
    }
    store_ticket_attachment(&state, &ticket.id, &req).await?;
    Ok(Json(state.db.get_ticket_by_id(&ticket.id)?))
}

async fn ticket_attachment_nas_path(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, att_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let ticket = state
        .db
        .get_ticket_by_id(&id)
        .or_else(|_| state.db.get_ticket_by_code(&id))?;
    match resolve_ticket_caller(&state.db, &headers)? {
        TicketCaller::Ti => {}
        TicketCaller::Agent { machine_id } => {
            if ticket.machine_id != machine_id {
                return Err(ApiError::Unauthorized);
            }
        }
        TicketCaller::Portal(user) => {
            if !portal_can_read_ticket(&state.db, &user, &ticket)? {
                return Err(ApiError::Unauthorized);
            }
        }
    }
    let att = state.db.get_ticket_attachment(&ticket.id, &att_id)?;
    let unc = tickets::nas_attachment_unc(&ticket, &att.stored_name);
    let unc_s = unc.to_string_lossy().replace('/', "\\");
    let folder = unc
        .parent()
        .map(|p| p.to_string_lossy().replace('/', "\\"))
        .unwrap_or_else(|| format!(r"\\FILE-SHARE\Portal\Chamados\{}\attachments", ticket.code));
    let file_url = format!("file:{}", unc_s.replace('\\', "/"));
    Ok(Json(serde_json::json!({
        "unc": unc_s,
        "folder": folder,
        "file_url": file_url,
        "filename": att.filename,
        "stored_name": att.stored_name,
        "code": ticket.code,
    })))
}

async fn download_ticket_attachment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, att_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    let ticket = state
        .db
        .get_ticket_by_id(&id)
        .or_else(|_| state.db.get_ticket_by_code(&id))?;
    match resolve_ticket_caller(&state.db, &headers)? {
        TicketCaller::Ti => {}
        TicketCaller::Agent { machine_id } => {
            if ticket.machine_id != machine_id {
                return Err(ApiError::Unauthorized);
            }
        }
        TicketCaller::Portal(user) => {
            if !portal_can_read_ticket(&state.db, &user, &ticket)? {
                return Err(ApiError::Unauthorized);
            }
        }
    }
    let att = state.db.get_ticket_attachment(&ticket.id, &att_id)?;
    let (bytes, used) = tickets::read_attachment_bytes(
        &state.chamados_root,
        &ticket,
        &att.stored_name,
        Some(&att.filename),
    )
    .map_err(|e| {
        tracing::warn!(
            "download attachment {} / {}: {e}",
            ticket.code,
            att.stored_name
        );
        ApiError::Internal(e)
    })?;
    tracing::debug!(
        "served attachment {} from {}",
        att.stored_name,
        used.display()
    );
    let safe_name = att.filename.replace('"', "").replace(['\r', '\n'], "_");
    let content_type = attachment_content_type(&att.filename);
    // inline: WebView2 / BelarcDesktop abre PNG/PDF no visualizador em vez de só "download"
    let disposition = format!("inline; filename=\"{safe_name}\"");
    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        bytes,
    ))
}

fn attachment_content_type(filename: &str) -> String {
    let ext = std::path::Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "txt" => "text/plain; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        "json" => "application/json",
        "html" | "htm" => "text/html; charset=utf-8",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "odt" => "application/vnd.oasis.opendocument.text",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    };
    mime.to_string()
}

async fn add_ticket_comment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<CommentRequest>,
) -> Result<Json<Ticket>, ApiError> {
    let ticket = state
        .db
        .get_ticket_by_id(&id)
        .or_else(|_| state.db.get_ticket_by_code(&id))?;
    let (role, name) = match resolve_ticket_caller(&state.db, &headers)? {
        TicketCaller::Ti => {
            let n = auth::session_username(&state.db, &headers).or(req.author_name.clone());
            ("ti", n)
        }
        TicketCaller::Agent { machine_id } => {
            if ticket.machine_id != machine_id {
                return Err(ApiError::Unauthorized);
            }
            (
                "user",
                req.author_name.clone().or(ticket.hostname_snapshot.clone()),
            )
        }
        TicketCaller::Portal(user) => {
            if !portal_can_read_ticket(&state.db, &user, &ticket)? {
                return Err(ApiError::Unauthorized);
            }
            (
                if portal_can_manage_ticket(&state.db, &user, &ticket)? {
                    "sector"
                } else {
                    "user"
                },
                Some(user.display_name),
            )
        }
    };
    if ticket.status == tickets::STATUS_DONE {
        return Err(ApiError::Conflict(
            "chamado fechado — nao aceita novos comentarios".into(),
        ));
    }
    let body = req.body.trim();
    if body.is_empty() && req.attachments.is_empty() {
        return Err(ApiError::Internal(
            "escreva uma mensagem ou anexe um arquivo".into(),
        ));
    }
    let comment_body = if body.is_empty() {
        "Anexo enviado na conversa."
    } else {
        body
    };
    state
        .db
        .add_ticket_comment(&ticket.id, role, name.as_deref(), comment_body)?;
    for attachment in &req.attachments {
        store_ticket_attachment(&state, &ticket.id, attachment).await?;
    }
    let mut updated = state.db.get_ticket_by_id(&ticket.id)?;
    mirror_ticket_to_nas(&state, &mut updated);
    Ok(Json(updated))
}

async fn add_checklist_item(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<ChecklistCreateRequest>,
) -> Result<Json<Ticket>, ApiError> {
    let ticket = state
        .db
        .get_ticket_by_id(&id)
        .or_else(|_| state.db.get_ticket_by_code(&id))?;
    match resolve_ticket_caller(&state.db, &headers)? {
        TicketCaller::Ti => {}
        TicketCaller::Agent { machine_id } => {
            if ticket.machine_id != machine_id {
                return Err(ApiError::Unauthorized);
            }
        }
        TicketCaller::Portal(user) => {
            if !portal_can_manage_ticket(&state.db, &user, &ticket)? {
                return Err(ApiError::Unauthorized);
            }
        }
    }
    if ticket.status == tickets::STATUS_DONE {
        return Err(ApiError::Conflict(
            "chamado concluido — checklist bloqueada".into(),
        ));
    }
    let label = req.label.trim();
    if label.is_empty() {
        return Err(ApiError::Internal("item da checklist vazio".into()));
    }
    state.db.add_checklist_item(&ticket.id, label)?;
    Ok(Json(state.db.get_ticket_by_id(&ticket.id)?))
}

async fn patch_checklist_item(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, item_id)): Path<(String, String)>,
    Json(req): Json<ChecklistPatchRequest>,
) -> Result<Json<Ticket>, ApiError> {
    let ticket = state
        .db
        .get_ticket_by_id(&id)
        .or_else(|_| state.db.get_ticket_by_code(&id))?;
    match resolve_ticket_caller(&state.db, &headers)? {
        TicketCaller::Ti => {}
        TicketCaller::Portal(user) if portal_can_manage_ticket(&state.db, &user, &ticket)? => {}
        _ => return Err(ApiError::Unauthorized),
    }
    if ticket.status == tickets::STATUS_DONE {
        return Err(ApiError::Conflict(
            "chamado concluido — checklist bloqueada".into(),
        ));
    }
    state
        .db
        .update_checklist_item(&ticket.id, &item_id, req.done, req.label.as_deref())?;
    Ok(Json(state.db.get_ticket_by_id(&ticket.id)?))
}

async fn delete_checklist_item(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, item_id)): Path<(String, String)>,
) -> Result<Json<Ticket>, ApiError> {
    let ticket = state
        .db
        .get_ticket_by_id(&id)
        .or_else(|_| state.db.get_ticket_by_code(&id))?;
    match resolve_ticket_caller(&state.db, &headers)? {
        TicketCaller::Ti => {}
        TicketCaller::Portal(user) if portal_can_manage_ticket(&state.db, &user, &ticket)? => {}
        _ => return Err(ApiError::Unauthorized),
    }
    if ticket.status == tickets::STATUS_DONE {
        return Err(ApiError::Conflict(
            "chamado concluido — checklist bloqueada".into(),
        ));
    }
    state.db.delete_checklist_item(&ticket.id, &item_id)?;
    Ok(Json(state.db.get_ticket_by_id(&ticket.id)?))
}

async fn close_ticket(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Option<Json<CloseTicketRequest>>,
) -> Result<Json<Ticket>, ApiError> {
    let ticket = state
        .db
        .get_ticket_by_id(&id)
        .or_else(|_| state.db.get_ticket_by_code(&id))?;
    match resolve_ticket_caller(&state.db, &headers)? {
        TicketCaller::Ti => {}
        TicketCaller::Portal(user) if portal_can_manage_ticket(&state.db, &user, &ticket)? => {}
        _ => return Err(ApiError::Unauthorized),
    }
    if ticket.status == tickets::STATUS_DONE {
        return Err(ApiError::Conflict("chamado ja esta concluido".into()));
    }
    let resolution = body
        .as_ref()
        .and_then(|b| b.resolution.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(ref body) = body {
        for attachment in &body.attachments {
            // O chamado ainda esta aberto aqui: o mesmo controle de tamanho,
            // nome seguro e espelho NAS usado na abertura e reutilizado.
            store_ticket_attachment(&state, &ticket.id, attachment).await?;
        }
    }
    let closed = state.db.close_ticket(&ticket.id, resolution)?;
    if let Some(res) = resolution {
        let closer_name = auth::session_username(&state.db, &headers).or_else(|| {
            auth::bearer_token(&headers)
                .and_then(|token| state.db.validate_portal_session(&token).ok().flatten())
                .map(|user| user.display_name)
        });
        let note = format!("[Resolução] {res}");
        let _ = state
            .db
            .add_ticket_comment(&closed.id, "sector", closer_name.as_deref(), &note);
    }
    let mut closed = state.db.get_ticket_by_id(&closed.id)?;
    mirror_ticket_to_nas(&state, &mut closed);
    Ok(Json(closed))
}

async fn rate_ticket(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<RateTicketRequest>,
) -> Result<Json<Ticket>, ApiError> {
    let ticket = state
        .db
        .get_ticket_by_id(&id)
        .or_else(|_| state.db.get_ticket_by_code(&id))?;
    match resolve_ticket_caller(&state.db, &headers)? {
        TicketCaller::Agent { machine_id } => {
            if ticket.machine_id != machine_id {
                return Err(ApiError::Unauthorized);
            }
        }
        TicketCaller::Ti => {
            return Err(ApiError::Internal(
                "avaliacao e feita pelo usuario do PC".into(),
            ));
        }
        TicketCaller::Portal(user) => {
            if user.role != "requester"
                || ticket.requester_user_id.as_deref() != Some(user.id.as_str())
            {
                return Err(ApiError::Unauthorized);
            }
        }
    }
    Ok(Json({
        let mut rated = state
            .db
            .rate_ticket(&ticket.id, req.rating, req.comment.as_deref())?;
        mirror_ticket_to_nas(&state, &mut rated);
        rated
    }))
}

async fn request_ticket_reopen(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<ReopenRequestBody>,
) -> Result<Json<Ticket>, ApiError> {
    let ticket = state
        .db
        .get_ticket_by_id(&id)
        .or_else(|_| state.db.get_ticket_by_code(&id))?;
    let author_name = match resolve_ticket_caller(&state.db, &headers)? {
        TicketCaller::Agent { machine_id } => {
            if ticket.machine_id != machine_id {
                return Err(ApiError::Unauthorized);
            }
            ticket.hostname_snapshot.clone()
        }
        TicketCaller::Ti => {
            return Err(ApiError::Internal(
                "pedido de reabertura e feito pelo usuario do PC".into(),
            ));
        }
        TicketCaller::Portal(user) => {
            if user.role != "requester"
                || ticket.requester_user_id.as_deref() != Some(user.id.as_str())
            {
                return Err(ApiError::Unauthorized);
            }
            user.display_name.into()
        }
    };
    if ticket.status != tickets::STATUS_DONE {
        return Err(ApiError::Conflict(
            "so e possivel pedir reabertura de chamado concluido".into(),
        ));
    }
    if ticket.reopen_pending {
        return Err(ApiError::Conflict(
            "ja existe pedido de reabertura pendente".into(),
        ));
    }
    let reason = req.reason.trim();
    if reason.is_empty() {
        return Err(ApiError::Internal("motivo obrigatorio".into()));
    }
    state.db.set_reopen_request(&ticket.id, reason)?;
    let body = format!("[Pedido de reabertura] {reason}");
    state
        .db
        .add_ticket_comment(&ticket.id, "user", author_name.as_deref(), &body)?;
    Ok(Json(state.db.get_ticket_by_id(&ticket.id)?))
}

async fn decide_ticket_reopen(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<ReopenDecideBody>,
) -> Result<Json<Ticket>, ApiError> {
    if !auth::is_ti_authenticated(&state.db, &headers) {
        return Err(ApiError::Unauthorized);
    }
    let ticket = state
        .db
        .get_ticket_by_id(&id)
        .or_else(|_| state.db.get_ticket_by_code(&id))?;
    if ticket.status != tickets::STATUS_DONE {
        return Err(ApiError::Conflict("chamado nao esta concluido".into()));
    }
    if !ticket.reopen_pending {
        return Err(ApiError::Conflict(
            "nao ha pedido de reabertura pendente".into(),
        ));
    }
    let reason = req.reason.trim();
    if reason.is_empty() {
        return Err(ApiError::Internal("motivo obrigatorio".into()));
    }
    let ti_name = auth::session_username(&state.db, &headers);
    if req.approve {
        let card_reason = ticket
            .reopen_reason
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(reason)
            .to_string();
        let updated = state.db.update_ticket(
            &ticket.id,
            None,
            None,
            Some(tickets::STATUS_OPEN),
            None,
            None,
        )?;
        let _ = state.db.set_last_reopen_reason(&updated.id, &card_reason);
        let body = format!("[Reabertura aprovada] {reason}");
        state
            .db
            .add_ticket_comment(&updated.id, "ti", ti_name.as_deref(), &body)?;
        let mut final_t = state.db.get_ticket_by_id(&updated.id)?;
        mirror_ticket_to_nas(&state, &mut final_t);
        Ok(Json(final_t))
    } else {
        state.db.clear_reopen_request(&ticket.id)?;
        let body = format!("[Reabertura recusada] {reason}");
        state
            .db
            .add_ticket_comment(&ticket.id, "ti", ti_name.as_deref(), &body)?;
        Ok(Json(state.db.get_ticket_by_id(&ticket.id)?))
    }
}

async fn reopen_ticket(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<ReopenBody>,
) -> Result<Json<Ticket>, ApiError> {
    if !auth::is_ti_authenticated(&state.db, &headers) {
        return Err(ApiError::Unauthorized);
    }
    let ticket = state
        .db
        .get_ticket_by_id(&id)
        .or_else(|_| state.db.get_ticket_by_code(&id))?;
    if ticket.status != tickets::STATUS_DONE {
        return Err(ApiError::Conflict("chamado nao esta concluido".into()));
    }
    let reason = req.reason.trim();
    if reason.is_empty() {
        return Err(ApiError::Internal("motivo obrigatorio".into()));
    }
    let status = match req.status.as_deref() {
        Some(s) => tickets::normalize_status(s)
            .filter(|st| *st != tickets::STATUS_DONE)
            .ok_or_else(|| ApiError::Internal("invalid status".into()))?,
        None => tickets::STATUS_OPEN,
    };
    let updated = state
        .db
        .update_ticket(&ticket.id, None, None, Some(status), None, None)?;
    let _ = state.db.set_last_reopen_reason(&updated.id, reason);
    let ti_name = auth::session_username(&state.db, &headers);
    let body = format!("[Reaberto pela TI] {reason}");
    state
        .db
        .add_ticket_comment(&updated.id, "ti", ti_name.as_deref(), &body)?;
    let mut final_t = state.db.get_ticket_by_id(&updated.id)?;
    mirror_ticket_to_nas(&state, &mut final_t);
    Ok(Json(final_t))
}

#[derive(Serialize)]
struct RegisterResponse {
    machine_id: String,
    message: String,
}

#[derive(Serialize)]
struct InventoryResponse {
    accepted: usize,
    changed_collectors: usize,
    health_score: u8,
}

#[derive(Serialize)]
struct MachineDetail {
    machine: MachineSummary,
    collectors: Vec<belarc_shared::CollectorResult>,
    highlight: MachineHighlight,
    admin: MachineAdmin,
    score_breakdown: crate::compliance::ScoreBreakdown,
}

#[derive(Serialize)]
struct CreateTokenResponse {
    token: String,
}

#[derive(Debug)]
pub enum ApiError {
    Db(crate::db::DbError),
    Internal(String),
    Unauthorized,
    Conflict(String),
}

impl From<crate::db::DbError> for ApiError {
    fn from(e: crate::db::DbError) -> Self {
        ApiError::Db(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        match self {
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized").into_response(),
            ApiError::Conflict(msg) => (StatusCode::CONFLICT, msg).into_response(),
            ApiError::Db(crate::db::DbError::InvalidToken) => {
                (StatusCode::UNAUTHORIZED, "invalid agent token").into_response()
            }
            ApiError::Db(crate::db::DbError::NotFound) => {
                (StatusCode::NOT_FOUND, "not found").into_response()
            }
            ApiError::Db(crate::db::DbError::InvalidInput(msg)) => {
                (StatusCode::BAD_REQUEST, msg).into_response()
            }
            ApiError::Db(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
            ApiError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response(),
        }
    }
}
