//! Helpdesk — tickets + pasta NAS Chamados.
//! Belarc Inventory — by Diogo.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::db::Database;

pub const STATUS_OPEN: &str = "open";
pub const STATUS_IN_PROGRESS: &str = "in_progress";
pub const STATUS_WAITING: &str = "waiting";
pub const STATUS_DONE: &str = "done";

pub const DEFAULT_CHAMADOS_ROOT: &str = r"\\FILE-SHARE\Portal\Chamados";
pub const FALLBACK_CHAMADOS_ROOT: &str = r"\\192.0.2.11\Portal\Chamados";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ticket {
    pub id: String,
    pub code: String,
    pub machine_id: String,
    pub hostname_snapshot: Option<String>,
    /// Nome do responsável no Cadastro TI no momento da abertura (imutável).
    pub owner_name_snapshot: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub priority: String,
    /// Setor que recebe o chamado. Chamados anteriores permanecem em TI.
    #[serde(default = "default_department")]
    pub department: String,
    /// Conta do portal que abriu o chamado (nulo para os fluxos legados agente/TI).
    pub requester_user_id: Option<String>,
    pub created_by: Option<String>,
    pub assignee: Option<String>,
    pub nas_path: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub closed_at: Option<String>,
    pub rating: Option<i32>,
    pub rating_comment: Option<String>,
    #[serde(default)]
    pub reopen_pending: bool,
    pub reopen_reason: Option<String>,
    pub reopen_requested_at: Option<String>,
    /// Motivo da última reabertura efetiva (persiste no card até nova reabertura).
    pub last_reopen_reason: Option<String>,
    /// Resolução opcional informada pela TI ao fechar.
    pub resolution: Option<String>,
    #[serde(default)]
    pub attachments: Vec<TicketAttachment>,
    #[serde(default)]
    pub comments: Vec<TicketComment>,
    #[serde(default)]
    pub checklist: Vec<ChecklistItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TicketAttachment {
    pub id: String,
    pub ticket_id: String,
    pub filename: String,
    pub stored_name: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TicketComment {
    pub id: String,
    pub ticket_id: String,
    pub author_role: String,
    pub author_name: Option<String>,
    pub body: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecklistItem {
    pub id: String,
    pub ticket_id: String,
    pub label: String,
    pub done: bool,
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateTicketRequest {
    pub title: String,
    pub description: Option<String>,
    pub priority: Option<String>,
    /// TI may open on behalf of a machine; agent auth ignores this.
    pub machine_id: Option<String>,
    /// ti | desenho | projeto | producao. Ausente preserva TI para clientes legados.
    pub department: Option<String>,
    #[serde(default)]
    pub attachments: Vec<AttachRequest>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTicketRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub assignee: Option<String>,
    pub department: Option<String>,
}

pub fn default_department() -> String {
    "ti".to_string()
}

pub fn normalize_department(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "ti" => Some("ti"),
        "desenho" => Some("desenho"),
        "projeto" => Some("projeto"),
        "producao" | "produção" => Some("producao"),
        _ => None,
    }
}

#[derive(Debug, Deserialize)]
pub struct AttachRequest {
    pub filename: String,
    /// Vazio = register-only (bytes ja no NAS pelo Belarc Chamado).
    #[serde(default)]
    pub content_base64: String,
    /// Nome no disco sob attachments/ (sanitizado). Se omitido, deriva de filename.
    #[serde(default)]
    pub stored_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CommentRequest {
    #[serde(default)]
    pub body: String,
    pub author_name: Option<String>,
    #[serde(default)]
    pub attachments: Vec<AttachRequest>,
}

#[derive(Debug, Deserialize)]
pub struct ChecklistCreateRequest {
    pub label: String,
}

#[derive(Debug, Deserialize)]
pub struct ChecklistPatchRequest {
    pub done: Option<bool>,
    pub label: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RateTicketRequest {
    pub rating: i32,
    pub comment: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReopenRequestBody {
    pub reason: String,
}

#[derive(Debug, Deserialize)]
pub struct ReopenDecideBody {
    pub approve: bool,
    pub reason: String,
}

#[derive(Debug, Deserialize)]
pub struct ReopenBody {
    pub reason: String,
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CloseTicketRequest {
    pub resolution: Option<String>,
    #[serde(default)]
    pub attachments: Vec<AttachRequest>,
}

/// meta.json no NAS (espelho completo).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NasTicketMeta {
    pub code: String,
    pub machine_id: String,
    #[serde(default)]
    pub hostname_snapshot: Option<String>,
    #[serde(default)]
    pub owner_name_snapshot: Option<String>,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    pub status: String,
    #[serde(default = "default_priority")]
    pub priority: String,
    #[serde(default)]
    pub created_by: Option<String>,
    #[serde(default)]
    pub assignee: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub closed_at: Option<String>,
    #[serde(default)]
    pub rating: Option<i32>,
    #[serde(default)]
    pub rating_comment: Option<String>,
    #[serde(default)]
    pub reopen_pending: bool,
    #[serde(default)]
    pub reopen_reason: Option<String>,
    #[serde(default)]
    pub reopen_requested_at: Option<String>,
    #[serde(default)]
    pub last_reopen_reason: Option<String>,
    #[serde(default)]
    pub resolution: Option<String>,
    #[serde(default)]
    pub nas_path: Option<String>,
    #[serde(default)]
    pub duration_min: Option<i64>,
    #[serde(default)]
    pub attachments_count: Option<usize>,
    #[serde(default)]
    pub comments_count: Option<usize>,
}

fn default_priority() -> String {
    "normal".into()
}

#[derive(Debug, Default, Serialize)]
pub struct NasSyncStats {
    pub scanned: usize,
    pub imported: usize,
    pub updated: usize,
    pub skipped: usize,
    pub errors: usize,
    pub export_ok: bool,
}

pub fn chamados_root() -> PathBuf {
    if let Ok(v) = std::env::var("BELARC_CHAMADOS_ROOT") {
        let p = PathBuf::from(v);
        if !p.as_os_str().is_empty() {
            return p;
        }
    }
    let primary = PathBuf::from(DEFAULT_CHAMADOS_ROOT);
    if primary.exists() || primary.parent().map(|p| p.exists()).unwrap_or(false) {
        return primary;
    }
    let fallback = PathBuf::from(FALLBACK_CHAMADOS_ROOT);
    if fallback.exists() || fallback.parent().map(|p| p.exists()).unwrap_or(false) {
        return fallback;
    }
    primary
}

/// Pasta local espelho — SYSTEM sempre le ProgramData.
pub fn local_chamados_files_root() -> PathBuf {
    if let Ok(data_dir) = std::env::var("BELARC_DATA_DIR") {
        let p = PathBuf::from(data_dir);
        if !p.as_os_str().is_empty() {
            return p.join("chamados-files");
        }
    }
    PathBuf::from(r"C:\ProgramData\BelarcInventoryServer\chamados-files")
}

/// Enfileira pasta local para o sync AtLogon (usuario) espelhar no NAS.
#[allow(dead_code)]
pub fn enqueue_pending_nas_sync(local_ticket_dir: &Path) {
    let data_dir = std::env::var("BELARC_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(r"C:\ProgramData\BelarcInventoryServer"));
    let path = data_dir.join("pending-nas-sync.json");
    let mut paths: Vec<String> = Vec::new();
    if let Ok(raw) = fs::read_to_string(&path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(arr) = v.get("paths").and_then(|p| p.as_array()) {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        paths.push(s.to_string());
                    }
                }
            }
        }
    }
    let s = local_ticket_dir.to_string_lossy().to_string();
    if !paths.iter().any(|p| p == &s) {
        paths.push(s);
    }
    let doc = serde_json::json!({ "paths": paths });
    if let Ok(bytes) = serde_json::to_vec_pretty(&doc) {
        let _ = fs::create_dir_all(&data_dir);
        let _ = fs::write(&path, bytes);
    }
}

/// Tenta somente o NAS (fonte da verdade). Nao cria pasta permanente no PC.
pub fn provision_ticket_folder(preferred_root: &Path, ticket: &Ticket) -> Result<PathBuf, String> {
    provision_nas_folder(preferred_root, ticket).map_err(|e| {
        format!("NAS indisponivel para o servidor (use sync usuario / Belarc Chamado): {e}")
    })
}

/// UNC canonico do chamado (sempre Portal\Chamados\<CODE>).
pub fn unc_ticket_dir(chamados_root: &Path, code: &str) -> PathBuf {
    chamados_root.join(code)
}

/// Remove pastas do chamado (NAS + espelho local). Best-effort — nao falha a API.
pub fn remove_ticket_folder(chamados_root: &Path, ticket: &Ticket) {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(ref p) = ticket.nas_path {
        dirs.push(PathBuf::from(p));
    }
    dirs.push(unc_ticket_dir(chamados_root, &ticket.code));
    dirs.push(PathBuf::from(FALLBACK_CHAMADOS_ROOT).join(&ticket.code));
    dirs.push(PathBuf::from(r"\\192.0.2.11\Portal\Chamados").join(&ticket.code));
    dirs.push(PathBuf::from(r"\\FILE-SHARE\Portal\Chamados").join(&ticket.code));
    dirs.push(local_chamados_files_root().join(&ticket.code));
    let mut seen = std::collections::HashSet::new();
    for dir in dirs {
        let key = dir.to_string_lossy().to_ascii_lowercase();
        if !seen.insert(key) {
            continue;
        }
        if dir.is_dir() {
            match fs::remove_dir_all(&dir) {
                Ok(()) => tracing::info!("removed ticket folder {}", dir.display()),
                Err(e) => tracing::warn!("remove ticket folder {}: {e}", dir.display()),
            }
        }
    }
}

/// Caminho UNC canonico do anexo no NAS (Explorer / app associado).
pub fn nas_attachment_unc(ticket: &Ticket, stored_name: &str) -> PathBuf {
    let name = Path::new(stored_name)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| stored_name.to_string());
    PathBuf::from(DEFAULT_CHAMADOS_ROOT)
        .join(&ticket.code)
        .join("attachments")
        .join(name)
}

/// Se nas_path aponta para chamados-files local, devolve UNC no NAS.
pub fn prefer_unc_path(chamados_root: &Path, ticket: &Ticket) -> PathBuf {
    if let Some(ref p) = ticket.nas_path {
        let s = p.replace('/', "\\");
        if s.to_ascii_lowercase().contains("chamados-files") || !s.starts_with("\\\\") {
            return unc_ticket_dir(chamados_root, &ticket.code);
        }
        return PathBuf::from(p);
    }
    unc_ticket_dir(chamados_root, &ticket.code)
}

/// Pastas-raiz do chamado (sem attachments/) — NAS primeiro; espelho local so fallback.
pub fn attachment_root_candidates(chamados_root: &Path, ticket: &Ticket) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    let push = |roots: &mut Vec<PathBuf>, p: PathBuf| {
        if !roots.iter().any(|r| r == &p) {
            roots.push(p);
        }
    };
    if let Some(ref p) = ticket.nas_path {
        let s = p.replace('/', "\\");
        if s.starts_with("\\\\") {
            push(&mut roots, PathBuf::from(p));
        }
    }
    push(&mut roots, prefer_unc_path(chamados_root, ticket));
    for alt in [
        PathBuf::from(r"\\FILE-SHARE\Portal\Chamados").join(&ticket.code),
        PathBuf::from(r"\\192.0.2.11\Portal\Chamados").join(&ticket.code),
        PathBuf::from(FALLBACK_CHAMADOS_ROOT).join(&ticket.code),
    ] {
        push(&mut roots, alt);
    }
    // Fallback legado — nunca destino de gravacao.
    push(&mut roots, local_chamados_files_root().join(&ticket.code));
    roots
}

/// Candidatos para ler um anexo (local primeiro; depois NAS UNC + IP).
#[allow(dead_code)]
pub fn attachment_file_candidates(
    chamados_root: &Path,
    ticket: &Ticket,
    stored_name: &str,
) -> Vec<PathBuf> {
    attachment_root_candidates(chamados_root, ticket)
        .into_iter()
        .map(|r| r.join("attachments").join(stored_name))
        .collect()
}

fn attachment_name_matches(
    candidate: &str,
    stored_name: &str,
    display_filename: Option<&str>,
) -> bool {
    if candidate.eq_ignore_ascii_case(stored_name) {
        return true;
    }
    if let Some(name) = display_filename.filter(|s| !s.is_empty()) {
        if candidate.eq_ignore_ascii_case(name) {
            return true;
        }
        if candidate == sanitize_filename(name) {
            return true;
        }
    }
    false
}

/// Lista attachments/ e devolve o primeiro arquivo que bate stored_name ou filename.
fn read_attachment_from_dirs(
    roots: &[PathBuf],
    stored_name: &str,
    display_filename: Option<&str>,
) -> Option<(Vec<u8>, PathBuf)> {
    for root in roots {
        let att_dir = root.join("attachments");
        let Ok(entries) = fs::read_dir(&att_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if attachment_name_matches(&name, stored_name, display_filename) {
                if let Ok(bytes) = fs::read(&path) {
                    return Some((bytes, path));
                }
            }
        }
    }
    None
}

/// Le bytes do anexo tentando varios caminhos; devolve (bytes, path usado) ou erro descritivo.
pub fn read_attachment_bytes(
    chamados_root: &Path,
    ticket: &Ticket,
    stored_name: &str,
    display_filename: Option<&str>,
) -> Result<(Vec<u8>, PathBuf), String> {
    let roots = attachment_root_candidates(chamados_root, ticket);
    let candidates = roots
        .iter()
        .map(|r| r.join("attachments").join(stored_name))
        .collect::<Vec<_>>();
    let mut last_err = String::from("arquivo nao encontrado");
    for path in &candidates {
        match fs::read(path) {
            Ok(bytes) => return Ok((bytes, path.clone())),
            Err(e) => {
                last_err = format!("{}: {}", path.display(), e);
                tracing::debug!("attachment read miss: {last_err}");
            }
        }
    }
    if let Some((bytes, path)) = read_attachment_from_dirs(&roots, stored_name, display_filename) {
        return Ok((bytes, path));
    }
    Err(format!(
        "anexo '{stored_name}' do {} inacessivel ({last_err}). \
         Verifique \\\\FILE-SHARE\\Portal\\Chamados e se a tarefa do servidor roda com usuario que acessa o NAS.",
        ticket.code
    ))
}

pub fn normalize_status(s: &str) -> Option<&'static str> {
    match s.trim().to_ascii_lowercase().as_str() {
        "open" | "aberto" => Some(STATUS_OPEN),
        "in_progress" | "em_andamento" | "andamento" => Some(STATUS_IN_PROGRESS),
        "waiting" | "aguardando" => Some(STATUS_WAITING),
        "done" | "concluido" | "concluído" | "closed" | "fechado" => Some(STATUS_DONE),
        _ => None,
    }
}

pub fn normalize_priority(s: &str) -> Option<&'static str> {
    match s.trim().to_ascii_lowercase().as_str() {
        "low" | "baixa" => Some("low"),
        "normal" | "media" | "média" => Some("normal"),
        "high" | "alta" => Some("high"),
        "urgent" | "urgente" => Some("high"),
        _ => None,
    }
}

/// Minutos entre abertura e fechamento (ou agora se ainda aberto).
pub fn duration_minutes(created_at: &str, closed_at: Option<&str>) -> Option<i64> {
    let start = chrono::DateTime::parse_from_rfc3339(created_at).ok()?;
    let end = if let Some(c) = closed_at.filter(|s| !s.is_empty()) {
        chrono::DateTime::parse_from_rfc3339(c).ok()?
    } else {
        chrono::Utc::now().fixed_offset()
    };
    Some((end - start).num_minutes().max(0))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let tmp = parent.join(format!(".{name}.tmp"));
    {
        let mut f = fs::File::create(&tmp).map_err(|e| e.to_string())?;
        f.write_all(bytes).map_err(|e| e.to_string())?;
        f.sync_all().ok();
    }
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(_) => {
            fs::copy(&tmp, path).map_err(|e| e.to_string())?;
            let _ = fs::remove_file(&tmp);
            Ok(())
        }
    }
}

/// Cria pasta do chamado no NAS (best-effort). Retorna path se ok.
pub fn provision_nas_folder(root: &Path, ticket: &Ticket) -> Result<PathBuf, String> {
    let dir = root.join(&ticket.code);
    let attachments = dir.join("attachments");
    fs::create_dir_all(&attachments).map_err(|e| e.to_string())?;

    let dur = duration_minutes(&ticket.created_at, ticket.closed_at.as_deref());
    let nas_path = dir.to_string_lossy().to_string();
    let meta = NasTicketMeta {
        code: ticket.code.clone(),
        machine_id: ticket.machine_id.clone(),
        hostname_snapshot: ticket.hostname_snapshot.clone(),
        owner_name_snapshot: ticket.owner_name_snapshot.clone(),
        title: ticket.title.clone(),
        description: ticket.description.clone(),
        status: ticket.status.clone(),
        priority: ticket.priority.clone(),
        created_by: ticket.created_by.clone(),
        assignee: ticket.assignee.clone(),
        created_at: ticket.created_at.clone(),
        updated_at: Some(ticket.updated_at.clone()),
        closed_at: ticket.closed_at.clone(),
        rating: ticket.rating,
        rating_comment: ticket.rating_comment.clone(),
        reopen_pending: ticket.reopen_pending,
        reopen_reason: ticket.reopen_reason.clone(),
        reopen_requested_at: ticket.reopen_requested_at.clone(),
        last_reopen_reason: ticket.last_reopen_reason.clone(),
        resolution: ticket.resolution.clone(),
        nas_path: Some(ticket.nas_path.clone().unwrap_or(nas_path.clone())),
        duration_min: dur,
        attachments_count: Some(ticket.attachments.len()),
        comments_count: Some(ticket.comments.len()),
    };
    let meta_bytes = serde_json::to_vec_pretty(&meta).map_err(|e| e.to_string())?;
    write_atomic(&dir.join("meta.json"), &meta_bytes)?;

    let md = format!(
        "# {}\n\n**Codigo:** {}\n**Host:** {}\n**Responsavel:** {}\n**Status:** {}\n**Prioridade:** {}\n**Assignee:** {}\n**Aberto:** {}\n**Atualizado:** {}\n**Fechado:** {}\n**Resolucao:** {}\n**Avaliacao:** {}\n\n{}\n",
        ticket.title,
        ticket.code,
        ticket.hostname_snapshot.as_deref().unwrap_or("-"),
        ticket.owner_name_snapshot.as_deref().unwrap_or("-"),
        ticket.status,
        ticket.priority,
        ticket.assignee.as_deref().unwrap_or("-"),
        ticket.created_at,
        ticket.updated_at,
        ticket.closed_at.as_deref().unwrap_or("-"),
        ticket.resolution.as_deref().unwrap_or("-"),
        ticket
            .rating
            .map(|r| r.to_string())
            .unwrap_or_else(|| "-".into()),
        ticket.description.as_deref().unwrap_or("")
    );
    write_atomic(dir.join("descricao.md").as_path(), md.as_bytes())?;
    Ok(dir)
}

/// Cabeçalhos + linhas do Excel de conformidade de chamados.
pub fn compliance_xlsx_rows(tickets: &[Ticket]) -> Vec<Vec<String>> {
    let mut rows = vec![vec![
        "Codigo".into(),
        "Titulo".into(),
        "Status".into(),
        "Prioridade".into(),
        "Host".into(),
        "Responsavel".into(),
        "Criado_por".into(),
        "Assignee".into(),
        "Aberto".into(),
        "Atualizado".into(),
        "Fechado".into(),
        "Duracao_min".into(),
        "Resolucao".into(),
        "Avaliacao".into(),
        "Comentario_avaliacao".into(),
        "Reopen_pendente".into(),
        "Motivo_reopen".into(),
        "Ultimo_motivo_reopen".into(),
        "Nas_path".into(),
        "Qtd_anexos".into(),
        "Qtd_comentarios".into(),
    ]];
    for t in tickets {
        let dur = duration_minutes(&t.created_at, t.closed_at.as_deref())
            .map(|d| d.to_string())
            .unwrap_or_default();
        rows.push(vec![
            t.code.clone(),
            t.title.clone(),
            t.status.clone(),
            t.priority.clone(),
            t.hostname_snapshot.clone().unwrap_or_default(),
            t.owner_name_snapshot.clone().unwrap_or_default(),
            t.created_by.clone().unwrap_or_default(),
            t.assignee.clone().unwrap_or_default(),
            t.created_at.clone(),
            t.updated_at.clone(),
            t.closed_at.clone().unwrap_or_default(),
            dur,
            t.resolution.clone().unwrap_or_default(),
            t.rating.map(|r| r.to_string()).unwrap_or_default(),
            t.rating_comment.clone().unwrap_or_default(),
            if t.reopen_pending {
                "sim".into()
            } else {
                "nao".into()
            },
            t.reopen_reason.clone().unwrap_or_default(),
            t.last_reopen_reason.clone().unwrap_or_default(),
            t.nas_path.clone().unwrap_or_default(),
            t.attachments.len().to_string(),
            t.comments.len().to_string(),
        ]);
    }
    rows
}

pub fn xlsx_bytes(rows: &[Vec<String>]) -> Result<Vec<u8>, String> {
    use std::io::Cursor;
    use zip::{write::FileOptions, CompressionMethod, ZipWriter};

    let esc = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let mut sheet = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>",
    );
    for (r, row) in rows.iter().enumerate() {
        sheet.push_str(&format!("<row r=\"{}\">", r + 1));
        for (c, val) in row.iter().enumerate() {
            let col = if c < 26 {
                ((b'A' + c as u8) as char).to_string()
            } else {
                // AA, AB… for >26 columns
                let hi = ((c / 26) - 1) as u8;
                let lo = (c % 26) as u8;
                format!("{}{}", (b'A' + hi) as char, (b'A' + lo) as char)
            };
            sheet.push_str(&format!(
                "<c r=\"{}{}\" t=\"inlineStr\"><is><t>{}</t></is></c>",
                col,
                r + 1,
                esc(val)
            ));
        }
        sheet.push_str("</row>");
    }
    sheet.push_str("</sheetData></worksheet>");
    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let opt: FileOptions<'_, ()> =
        FileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, content) in [
        (
            "[Content_Types].xml",
            "<?xml version=\"1.0\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/><Override PartName=\"/xl/worksheets/sheet1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/></Types>".to_string(),
        ),
        (
            "_rels/.rels",
            "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>".to_string(),
        ),
        (
            "xl/workbook.xml",
            "<?xml version=\"1.0\"?><workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheets><sheet name=\"Chamados\" sheetId=\"1\" r:id=\"rId1\"/></sheets></workbook>".to_string(),
        ),
        (
            "xl/_rels/workbook.xml.rels",
            "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet1.xml\"/></Relationships>".to_string(),
        ),
        ("xl/worksheets/sheet1.xml", sheet),
    ] {
        zip.start_file(name, opt).map_err(|e| e.to_string())?;
        zip.write_all(content.as_bytes()).map_err(|e| e.to_string())?;
    }
    zip.finish()
        .map_err(|e| e.to_string())
        .map(|c| c.into_inner())
}

/// Regenera `_index.json` + `belarc-chamados.xlsx` na raiz de Chamados.
/// Tambem espelha em `%BELARC_DATA_DIR%/chamados-export` (sempre acessivel ao servidor).
pub fn sync_chamados_export(root: &Path, tickets: &[Ticket]) -> Result<(), String> {
    let rows = compliance_xlsx_rows(tickets);
    let xlsx = xlsx_bytes(&rows)?;
    let index: Vec<serde_json::Value> = tickets
        .iter()
        .map(|t| {
            serde_json::json!({
                "code": t.code,
                "title": t.title,
                "status": t.status,
                "priority": t.priority,
                "hostname": t.hostname_snapshot,
                "owner": t.owner_name_snapshot,
                "assignee": t.assignee,
                "updated_at": t.updated_at,
                "closed_at": t.closed_at,
                "rating": t.rating,
                "duration_min": duration_minutes(&t.created_at, t.closed_at.as_deref()),
                "nas_path": t.nas_path,
            })
        })
        .collect();
    let index_doc = serde_json::json!({
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "count": tickets.len(),
        "tickets": index,
    });
    let index_bytes = serde_json::to_vec_pretty(&index_doc).map_err(|e| e.to_string())?;

    // Local mirror (SYSTEM sempre consegue)
    if let Ok(data_dir) = std::env::var("BELARC_DATA_DIR") {
        let local = PathBuf::from(data_dir).join("chamados-export");
        let _ = fs::create_dir_all(&local);
        let _ = write_atomic(&local.join("_index.json"), &index_bytes);
        let _ = write_atomic(&local.join("belarc-chamados.xlsx"), &xlsx);
    }

    // NAS (best-effort — SYSTEM pode nao ter credencial de rede)
    match fs::create_dir_all(root) {
        Ok(()) => {
            write_atomic(&root.join("_index.json"), &index_bytes)?;
            write_atomic(&root.join("belarc-chamados.xlsx"), &xlsx)?;
            Ok(())
        }
        Err(e) => {
            tracing::warn!(
                "NAS chamados export indisponivel ({}): {e} — mantido espelho local",
                root.display()
            );
            Ok(())
        }
    }
}

fn rfc3339_newer(a: &str, b: &str) -> bool {
    match (
        chrono::DateTime::parse_from_rfc3339(a),
        chrono::DateTime::parse_from_rfc3339(b),
    ) {
        (Ok(da), Ok(db)) => da > db,
        _ => a > b,
    }
}

/// Importa pastas CHM-* do NAS para o SQLite (NAS vence se updated_at mais novo).
pub fn import_from_nas(db: &Database, root: &Path) -> Result<NasSyncStats, String> {
    let mut stats = NasSyncStats::default();

    if root.exists() {
        match fs::read_dir(root) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }
                    let name = match path.file_name().and_then(|n| n.to_str()) {
                        Some(n) if n.starts_with("CHM-") => n.to_string(),
                        _ => continue,
                    };
                    // Pastas *-PEND-* sao outbox do app do usuario; nao entram no Kanban
                    // (evita duplicar quando o chamado ja recebeu codigo definitivo).
                    if name.to_ascii_uppercase().contains("-PEND-") {
                        tracing::debug!("chamados import skip outbox {}", name);
                        stats.skipped += 1;
                        continue;
                    }
                    stats.scanned += 1;
                    let meta_path = path.join("meta.json");
                    let meta_raw = match fs::read_to_string(&meta_path) {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::warn!("chamados import {}: meta.json: {e}", name);
                            stats.errors += 1;
                            continue;
                        }
                    };
                    let mut meta: NasTicketMeta = match serde_json::from_str(&meta_raw) {
                        Ok(m) => m,
                        Err(e) => {
                            tracing::warn!("chamados import {}: meta parse: {e}", name);
                            stats.errors += 1;
                            continue;
                        }
                    };
                    if meta.code.trim().is_empty() {
                        meta.code = name.clone();
                    }
                    if meta.updated_at.as_deref().unwrap_or("").is_empty() {
                        meta.updated_at = Some(meta.created_at.clone());
                    }
                    if meta.description.as_deref().unwrap_or("").trim().is_empty() {
                        if let Ok(md) = fs::read_to_string(path.join("descricao.md")) {
                            if let Some(body) = md.split("\n\n").last() {
                                let body = body.trim();
                                if !body.is_empty() && !body.starts_with('#') {
                                    meta.description = Some(body.to_string());
                                }
                            }
                        }
                    }

                    let nas_updated = meta
                        .updated_at
                        .clone()
                        .unwrap_or_else(|| meta.created_at.clone());
                    match db.find_ticket_by_code(&meta.code) {
                        Ok(Some(existing)) => {
                            if rfc3339_newer(&nas_updated, &existing.updated_at) {
                                match db.apply_nas_ticket_meta(&existing.id, &meta, &path) {
                                    Ok(_) => {
                                        sync_attachments_from_dir(db, &existing.id, &path);
                                        stats.updated += 1;
                                    }
                                    Err(e) => {
                                        tracing::warn!("chamados update {}: {e}", meta.code);
                                        stats.errors += 1;
                                    }
                                }
                            } else {
                                sync_attachments_from_dir(db, &existing.id, &path);
                                stats.skipped += 1;
                            }
                        }
                        Ok(None) => match db.import_ticket_from_nas(&meta, &path) {
                            Ok(t) => {
                                sync_attachments_from_dir(db, &t.id, &path);
                                stats.imported += 1;
                            }
                            Err(e) => {
                                tracing::warn!("chamados import {}: {e}", meta.code);
                                stats.errors += 1;
                            }
                        },
                        Err(e) => {
                            tracing::warn!("chamados lookup {}: {e}", meta.code);
                            stats.errors += 1;
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("NAS chamados read_dir {}: {e}", root.display());
            }
        }
    } else {
        tracing::warn!(
            "NAS chamados root indisponivel ({}): import adiado; gerando espelho local",
            root.display()
        );
    }

    // Sempre regenera Excel/_index (local + NAS best-effort)
    match db.list_tickets(None, None) {
        Ok(list) => match sync_chamados_export(root, &list) {
            Ok(()) => stats.export_ok = true,
            Err(e) => {
                tracing::warn!("chamados export apos import: {e}");
                stats.export_ok = false;
            }
        },
        Err(e) => {
            tracing::warn!("chamados list apos import: {e}");
            stats.export_ok = false;
        }
    }
    Ok(stats)
}

pub fn sync_attachments_from_dir(db: &Database, ticket_id: &str, nas_dir: &Path) {
    let att_dir = nas_dir.join("attachments");
    let Ok(entries) = fs::read_dir(&att_dir) else {
        return;
    };
    let existing = db.list_ticket_attachments(ticket_id).unwrap_or_default();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let stored = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if existing.iter().any(|a| a.stored_name == stored) {
            continue;
        }
        if let Err(e) = db.add_ticket_attachment(ticket_id, &stored, &stored) {
            tracing::warn!("chamados attach meta {stored}: {e}");
        }
    }
}

pub fn write_attachment_file(
    nas_dir: &Path,
    filename: &str,
    bytes: &[u8],
) -> Result<String, String> {
    // Mesmo esquema do Belarc Chamado (sanitize only) — download e register batem.
    let safe = sanitize_filename(filename);
    let attachments = nas_dir.join("attachments");
    fs::create_dir_all(&attachments).map_err(|e| e.to_string())?;
    let dest = attachments.join(&safe);
    fs::write(&dest, bytes).map_err(|e| e.to_string())?;
    Ok(safe)
}

pub fn sanitize_filename(name: &str) -> String {
    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    if base.is_empty() {
        "anexo.bin".into()
    } else {
        base
    }
}

pub fn decode_base64(input: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(cleaned.as_bytes())
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(cleaned.as_bytes()))
        .map_err(|e| e.to_string())
}
