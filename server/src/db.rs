use std::path::Path;
use std::sync::Mutex;

use belarc_shared::{
    hash_json, AlertRecord, AlertSeverity, CollectorResult, HeartbeatPayload, IncidentRecord,
    InventoryPayload, MachineStatus, MachineSummary, RegisterPayload,
};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DbError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("invalid token")]
    InvalidToken,
    #[error("machine not found")]
    NotFound,
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

pub struct Database {
    pub(crate) conn: Mutex<Connection>,
}

/// Conta restrita do portal de chamados. Ela nao compartilha a sessao nem os
/// privilegios do login TI, que continua sendo a credencial administrativa.
#[derive(Debug, Clone)]
pub struct PortalUser {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub password_hash: String,
    pub role: String,
    pub department: Option<String>,
}

/// Setor que pode receber chamados. O identificador (`slug`) e estavel para
/// que renomear o setor nao reescreva o historico dos chamados.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TicketDepartment {
    pub slug: String,
    pub name: String,
    pub active: bool,
    pub system: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self, DbError> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn migrate(&self) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS machines (
                id TEXT PRIMARY KEY,
                agent_token TEXT UNIQUE NOT NULL,
                hostname TEXT NOT NULL,
                serial TEXT,
                machine_uuid TEXT,
                mac_primary TEXT,
                machine_fingerprint TEXT,
                status TEXT NOT NULL DEFAULT 'offline',
                logged_user TEXT,
                ip_address TEXT,
                uptime_seconds INTEGER,
                last_boot TEXT,
                health_score INTEGER,
                first_seen TEXT NOT NULL,
                last_seen TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS heartbeats (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                machine_id TEXT NOT NULL REFERENCES machines(id),
                logged_user TEXT,
                ip_address TEXT,
                uptime_seconds INTEGER,
                created_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_heartbeats_machine ON heartbeats(machine_id, created_at);

            CREATE TABLE IF NOT EXISTS inventory_snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                machine_id TEXT NOT NULL REFERENCES machines(id),
                tier TEXT NOT NULL,
                collector_name TEXT NOT NULL,
                collector_version TEXT NOT NULL,
                data_json TEXT NOT NULL,
                data_hash TEXT NOT NULL,
                collected_at TEXT NOT NULL,
                UNIQUE(machine_id, collector_name)
            );

            CREATE TABLE IF NOT EXISTS inventory_deltas (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                machine_id TEXT NOT NULL REFERENCES machines(id),
                collector_name TEXT NOT NULL,
                field_path TEXT NOT NULL,
                old_value TEXT,
                new_value TEXT,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS alerts (
                id TEXT PRIMARY KEY,
                machine_id TEXT NOT NULL REFERENCES machines(id),
                severity TEXT NOT NULL,
                category TEXT NOT NULL,
                message TEXT NOT NULL,
                created_at TEXT NOT NULL,
                resolved INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS agent_tokens (
                token TEXT PRIMARY KEY,
                hostname TEXT,
                created_at TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                consumed_at TEXT,
                active INTEGER NOT NULL DEFAULT 1
            );

            CREATE TABLE IF NOT EXISTS incident_events (
                id TEXT PRIMARY KEY,
                machine_id TEXT NOT NULL REFERENCES machines(id) ON DELETE CASCADE,
                category TEXT NOT NULL,
                severity TEXT NOT NULL,
                title TEXT NOT NULL,
                message TEXT NOT NULL,
                metric_value REAL,
                threshold REAL,
                recommendation TEXT,
                source_collector TEXT NOT NULL,
                observed_at TEXT NOT NULL,
                first_seen TEXT NOT NULL,
                last_seen TEXT NOT NULL,
                occurrence_count INTEGER NOT NULL DEFAULT 1
            );

            CREATE INDEX IF NOT EXISTS idx_incidents_machine ON incident_events(machine_id, last_seen DESC);

            CREATE TABLE IF NOT EXISTS machine_admin (
                machine_id TEXT PRIMARY KEY REFERENCES machines(id) ON DELETE CASCADE,
                owner_name TEXT,
                ramal TEXT,
                primary_email TEXT,
                network_cable TEXT,
                cybersul_user TEXT,
                cybersul_password TEXT,
                nas_user TEXT,
                nas_password TEXT,
                notes TEXT,
                maintenance_status TEXT,
                maintenance_notes TEXT,
                ti_comments TEXT,
                updated_at TEXT
            );

            CREATE TABLE IF NOT EXISTS ti_users (
                id TEXT PRIMARY KEY,
                username TEXT UNIQUE NOT NULL,
                password_hash TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS ti_sessions (
                token TEXT PRIMARY KEY,
                user_id TEXT NOT NULL REFERENCES ti_users(id) ON DELETE CASCADE,
                expires_at TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_ti_sessions_expires ON ti_sessions(expires_at);

            CREATE TABLE IF NOT EXISTS tickets (
                id TEXT PRIMARY KEY,
                code TEXT UNIQUE NOT NULL,
                machine_id TEXT NOT NULL REFERENCES machines(id) ON DELETE CASCADE,
                hostname_snapshot TEXT,
                owner_name_snapshot TEXT,
                title TEXT NOT NULL,
                description TEXT,
                status TEXT NOT NULL,
                priority TEXT NOT NULL DEFAULT 'normal',
                created_by TEXT,
                assignee TEXT,
                nas_path TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_tickets_status ON tickets(status, updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_tickets_machine ON tickets(machine_id, created_at DESC);

            CREATE TABLE IF NOT EXISTS portal_users (
                id TEXT PRIMARY KEY,
                username TEXT UNIQUE NOT NULL,
                display_name TEXT NOT NULL,
                password_hash TEXT NOT NULL,
                role TEXT NOT NULL CHECK(role IN ('requester','sector_agent')),
                department TEXT,
                active INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS portal_sessions (
                token TEXT PRIMARY KEY,
                user_id TEXT NOT NULL REFERENCES portal_users(id) ON DELETE CASCADE,
                expires_at TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_portal_sessions_expires ON portal_sessions(expires_at);
            CREATE TABLE IF NOT EXISTS portal_user_machines (
                user_id TEXT NOT NULL REFERENCES portal_users(id) ON DELETE CASCADE,
                machine_id TEXT NOT NULL REFERENCES machines(id) ON DELETE CASCADE,
                PRIMARY KEY (user_id, machine_id)
            );
            CREATE TABLE IF NOT EXISTS machine_ticket_departments (
                machine_id TEXT NOT NULL REFERENCES machines(id) ON DELETE CASCADE,
                department TEXT NOT NULL,
                PRIMARY KEY (machine_id, department)
            );
            CREATE INDEX IF NOT EXISTS idx_machine_ticket_departments_department
                ON machine_ticket_departments(department, machine_id);
            CREATE TABLE IF NOT EXISTS portal_user_departments (
                user_id TEXT NOT NULL REFERENCES portal_users(id) ON DELETE CASCADE,
                department TEXT NOT NULL,
                PRIMARY KEY (user_id, department)
            );
            CREATE TABLE IF NOT EXISTS ticket_departments (
                slug TEXT PRIMARY KEY,
                name TEXT NOT NULL COLLATE NOCASE UNIQUE,
                active INTEGER NOT NULL DEFAULT 1,
                system INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS ticket_attachments (
                id TEXT PRIMARY KEY,
                ticket_id TEXT NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
                filename TEXT NOT NULL,
                stored_name TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS ticket_comments (
                id TEXT PRIMARY KEY,
                ticket_id TEXT NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
                author_role TEXT NOT NULL,
                author_name TEXT,
                body TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_ticket_comments_ticket
                ON ticket_comments(ticket_id, created_at);

            CREATE TABLE IF NOT EXISTS ticket_checklist (
                id TEXT PRIMARY KEY,
                ticket_id TEXT NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
                label TEXT NOT NULL,
                done INTEGER NOT NULL DEFAULT 0,
                sort_order INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_ticket_checklist_ticket
                ON ticket_checklist(ticket_id, sort_order);

            CREATE TABLE IF NOT EXISTS directory_people (
                id TEXT PRIMARY KEY,
                name_key TEXT UNIQUE NOT NULL,
                display_name TEXT NOT NULL,
                ramal TEXT,
                notes TEXT,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS directory_person_machines (
                id TEXT PRIMARY KEY,
                person_id TEXT NOT NULL REFERENCES directory_people(id) ON DELETE CASCADE,
                machine_id TEXT REFERENCES machines(id) ON DELETE SET NULL,
                hostname TEXT,
                pc_code TEXT,
                mouse_pad TEXT,
                mouse TEXT,
                keyboard TEXT,
                cabinet TEXT,
                power_supply TEXT,
                network_cable TEXT,
                UNIQUE(person_id, hostname)
            );

            CREATE INDEX IF NOT EXISTS idx_dir_person_machines_person
                ON directory_person_machines(person_id);
            CREATE INDEX IF NOT EXISTS idx_dir_person_machines_host
                ON directory_person_machines(hostname);

            CREATE TABLE IF NOT EXISTS directory_emails (
                id TEXT PRIMARY KEY,
                address TEXT UNIQUE NOT NULL,
                password TEXT,
                notes TEXT,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS directory_email_members (
                email_id TEXT NOT NULL REFERENCES directory_emails(id) ON DELETE CASCADE,
                person_id TEXT NOT NULL REFERENCES directory_people(id) ON DELETE CASCADE,
                PRIMARY KEY (email_id, person_id)
            );

            CREATE INDEX IF NOT EXISTS idx_dir_email_members_person
                ON directory_email_members(person_id);

            CREATE TABLE IF NOT EXISTS directory_nas_accounts (
                person_id TEXT PRIMARY KEY REFERENCES directory_people(id) ON DELETE CASCADE,
                login TEXT,
                password TEXT,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS directory_nas_shares (
                id TEXT PRIMARY KEY,
                person_id TEXT NOT NULL REFERENCES directory_people(id) ON DELETE CASCADE,
                share_key TEXT NOT NULL,
                share_label TEXT,
                allowed INTEGER NOT NULL DEFAULT 0,
                UNIQUE(person_id, share_key)
            );

            CREATE TABLE IF NOT EXISTS directory_cybersul (
                person_id TEXT PRIMARY KEY REFERENCES directory_people(id) ON DELETE CASCADE,
                login TEXT,
                password TEXT,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS directory_anydesk (
                id TEXT PRIMARY KEY,
                person_id TEXT REFERENCES directory_people(id) ON DELETE SET NULL,
                hostname TEXT,
                pc_code TEXT,
                alias TEXT,
                anydesk_id TEXT,
                password TEXT,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS directory_cables (
                id TEXT PRIMARY KEY,
                person_id TEXT REFERENCES directory_people(id) ON DELETE SET NULL,
                cable_id TEXT,
                origem TEXT,
                porta TEXT,
                pc_code TEXT,
                notes TEXT,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS directory_software (
                id TEXT PRIMARY KEY,
                person_id TEXT REFERENCES directory_people(id) ON DELETE SET NULL,
                pc_code TEXT,
                hostname TEXT,
                data_json TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS directory_server_accounts (
                id TEXT PRIMARY KEY,
                login TEXT NOT NULL,
                password TEXT,
                notes TEXT,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS directory_impacta (
                id TEXT PRIMARY KEY,
                person_id TEXT REFERENCES directory_people(id) ON DELETE SET NULL,
                usuario TEXT,
                ramal TEXT,
                senha TEXT,
                ip TEXT,
                porta TEXT,
                updated_at TEXT NOT NULL
            );

            "#,
        )?;

        let _ = conn.execute("ALTER TABLE tickets ADD COLUMN closed_at TEXT", []);
        let _ = conn.execute("ALTER TABLE tickets ADD COLUMN rating INTEGER", []);
        let _ = conn.execute("ALTER TABLE tickets ADD COLUMN rating_comment TEXT", []);
        let _ = conn.execute(
            "ALTER TABLE tickets ADD COLUMN owner_name_snapshot TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE tickets ADD COLUMN reopen_pending INTEGER DEFAULT 0",
            [],
        );
        let _ = conn.execute("ALTER TABLE tickets ADD COLUMN reopen_reason TEXT", []);
        let _ = conn.execute(
            "ALTER TABLE tickets ADD COLUMN reopen_requested_at TEXT",
            [],
        );
        let _ = conn.execute("ALTER TABLE tickets ADD COLUMN last_reopen_reason TEXT", []);
        let _ = conn.execute("ALTER TABLE tickets ADD COLUMN resolution TEXT", []);
        // Chamados antigos: preenche snapshot vazio com o responsavel atual do cadastro (uma vez).
        let _ = conn.execute(
            "UPDATE tickets
             SET owner_name_snapshot = (
               SELECT TRIM(a.owner_name) FROM machine_admin a
               WHERE a.machine_id = tickets.machine_id
                 AND a.owner_name IS NOT NULL
                 AND TRIM(a.owner_name) != ''
             )
             WHERE (owner_name_snapshot IS NULL OR TRIM(owner_name_snapshot) = '')
               AND EXISTS (
                 SELECT 1 FROM machine_admin a
                 WHERE a.machine_id = tickets.machine_id
                   AND a.owner_name IS NOT NULL
                   AND TRIM(a.owner_name) != ''
               )",
            [],
        );

        let _ = conn.execute(
            "ALTER TABLE machine_admin ADD COLUMN maintenance_status TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE machine_admin ADD COLUMN maintenance_notes TEXT",
            [],
        );
        let _ = conn.execute("ALTER TABLE machine_admin ADD COLUMN ti_comments TEXT", []);

        // Portal de chamados por setor. Defaults preservam integralmente os
        // chamados existentes e os clientes legados (agente/desktop atual).
        let _ = conn.execute(
            "ALTER TABLE tickets ADD COLUMN department TEXT NOT NULL DEFAULT 'ti'",
            [],
        );
        let _ = conn.execute("ALTER TABLE tickets ADD COLUMN requester_user_id TEXT", []);
        let _ = conn.execute(
            "UPDATE tickets SET department='ti' WHERE department IS NULL OR TRIM(department)=''",
            [],
        );
        let _ = conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_tickets_department ON tickets(department, updated_at DESC); CREATE INDEX IF NOT EXISTS idx_tickets_requester ON tickets(requester_user_id, updated_at DESC);");
        let _ = conn.execute(
            "ALTER TABLE machine_admin ADD COLUMN ticket_default_department TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE machine_admin ADD COLUMN ticket_attendant_department TEXT",
            [],
        );
        // Migra o único setor da versão intermediária para a lista de setores
        // recebidos; não remove a coluna anterior para preservar rollback local.
        let _ = conn.execute_batch("INSERT OR IGNORE INTO machine_ticket_departments (machine_id, department) SELECT machine_id, ticket_attendant_department FROM machine_admin WHERE ticket_attendant_department IN ('ti','desenho','projeto','producao');");
        let now = Utc::now().to_rfc3339();
        for (slug, name, system) in [
            ("ti", "TI", 1_i64),
            ("desenho", "Desenho", 1),
            ("projeto", "Projeto", 1),
            ("producao", "Produção", 1),
        ] {
            let _ = conn.execute(
                "INSERT OR IGNORE INTO ticket_departments (slug,name,active,system,created_at,updated_at) VALUES (?1,?2,1,?3,?4,?4)",
                params![slug, name, system, now],
            );
        }
        // Bases antigas podem ter chamados de um setor que ainda nao existia
        // na configuracao. Mantemos esse setor visivel e ativo, sem perder
        // historico durante a atualizacao.
        let _ = conn.execute(
            "INSERT OR IGNORE INTO ticket_departments (slug,name,active,system,created_at,updated_at)
             SELECT DISTINCT lower(trim(department)), trim(department), 1, 0, ?1, ?1
             FROM tickets WHERE department IS NOT NULL AND trim(department)<>''",
            [now],
        );

        let _ = conn.execute(
            "ALTER TABLE machines ADD COLUMN machine_fingerprint TEXT",
            [],
        );
        let _ = conn.execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_machines_fingerprint
             ON machines(machine_fingerprint) WHERE machine_fingerprint IS NOT NULL;",
        );

        // Diretório hub — colunas/tabelas novas (idempotente)
        let _ = conn.execute(
            "ALTER TABLE directory_emails ADD COLUMN provider TEXT DEFAULT 'other'",
            [],
        );
        let _ = conn.execute("ALTER TABLE directory_cables ADD COLUMN switch_id TEXT", []);
        let _ = conn.execute("ALTER TABLE directory_cables ADD COLUMN hostname TEXT", []);
        let _ = conn.execute(
            "ALTER TABLE directory_cables ADD COLUMN machine_id TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE directory_server_accounts ADD COLUMN description TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE directory_server_accounts ADD COLUMN tags TEXT DEFAULT ''",
            [],
        );
        let _ = conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS directory_mail_settings (
                id TEXT PRIMARY KEY DEFAULT 'default',
                skymail_imap_host TEXT,
                skymail_imap_port INTEGER,
                skymail_imap_ssl INTEGER DEFAULT 1,
                skymail_smtp_host TEXT,
                skymail_smtp_port INTEGER,
                skymail_smtp_ssl INTEGER DEFAULT 1,
                skymail_pop_host TEXT,
                skymail_pop_port INTEGER,
                skymail_pop_ssl INTEGER DEFAULT 1,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS directory_share_catalog (
                share_key TEXT PRIMARY KEY,
                share_label TEXT,
                sort_order INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS directory_switches (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                port_count INTEGER NOT NULL DEFAULT 24,
                notes TEXT,
                updated_at TEXT NOT NULL
            );",
        );
        // Diretório hub v2 — PC-01 / autofill
        let _ = conn.execute("ALTER TABLE directory_switches ADD COLUMN model TEXT", []);
        let _ = conn.execute(
            "ALTER TABLE directory_anydesk ADD COLUMN machine_id TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE directory_software ADD COLUMN machine_id TEXT",
            [],
        );
        let _ = conn.execute("ALTER TABLE directory_software ADD COLUMN windows TEXT", []);
        let _ = conn.execute(
            "ALTER TABLE directory_software ADD COLUMN win_version TEXT",
            [],
        );
        let _ = conn.execute("ALTER TABLE directory_software ADD COLUMN office TEXT", []);
        let _ = conn.execute(
            "ALTER TABLE directory_software ADD COLUMN office_year TEXT",
            [],
        );
        let _ = conn.execute("ALTER TABLE directory_software ADD COLUMN eset TEXT", []);
        let _ = conn.execute("ALTER TABLE directory_software ADD COLUMN fusion TEXT", []);
        let _ = conn.execute(
            "ALTER TABLE directory_software ADD COLUMN digital_vesper TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE directory_software ADD COLUMN digital_ventrio TEXT",
            [],
        );
        let _ = conn.execute("ALTER TABLE directory_software ADD COLUMN ip TEXT", []);
        let _ = conn.execute("ALTER TABLE directory_software ADD COLUMN usuario TEXT", []);
        // Instalações antigas não possuíam expiração/consumo para a matrícula.
        // As tentativas podem falhar em bancos novos, por isso são deliberadamente
        // idempotentes durante a atualização de esquema.
        let _ = conn.execute("ALTER TABLE agent_tokens ADD COLUMN expires_at TEXT", []);
        let _ = conn.execute("ALTER TABLE agent_tokens ADD COLUMN consumed_at TEXT", []);
        let _ = conn.execute(
            "UPDATE agent_tokens SET expires_at = COALESCE(expires_at, created_at)",
            [],
        );
        // Promove chaves do JSON legado (Infos CSV) para colunas tipadas
        let _ = conn.execute_batch(
            r#"
            UPDATE directory_software SET windows = COALESCE(NULLIF(windows,''), json_extract(data_json, '$.Windows'), json_extract(data_json, '$.windows')) WHERE windows IS NULL OR windows = '';
            UPDATE directory_software SET win_version = COALESCE(NULLIF(win_version,''), json_extract(data_json, '$.Win.Versão'), json_extract(data_json, '$.["Win.Versão"]'), json_extract(data_json, '$.win_version')) WHERE win_version IS NULL OR win_version = '';
            UPDATE directory_software SET office = COALESCE(NULLIF(office,''), json_extract(data_json, '$.Office'), json_extract(data_json, '$.office')) WHERE office IS NULL OR office = '';
            UPDATE directory_software SET office_year = COALESCE(NULLIF(office_year,''), json_extract(data_json, '$.Off.Ano'), json_extract(data_json, '$.["Off.Ano"]'), json_extract(data_json, '$.office_year')) WHERE office_year IS NULL OR office_year = '';
            UPDATE directory_software SET eset = COALESCE(NULLIF(eset,''), json_extract(data_json, '$.Eset'), json_extract(data_json, '$.eset')) WHERE eset IS NULL OR eset = '';
            UPDATE directory_software SET fusion = COALESCE(NULLIF(fusion,''), json_extract(data_json, '$.Fusion'), json_extract(data_json, '$.fusion')) WHERE fusion IS NULL OR fusion = '';
            UPDATE directory_software SET digital_vesper = COALESCE(NULLIF(digital_vesper,''), json_extract(data_json, '$.Digital.Northwind'), json_extract(data_json, '$.["Digital.Northwind"]')) WHERE digital_vesper IS NULL OR digital_vesper = '';
            UPDATE directory_software SET digital_ventrio = COALESCE(NULLIF(digital_ventrio,''), json_extract(data_json, '$.Digital.Fabrikam'), json_extract(data_json, '$.["Digital.Fabrikam"]')) WHERE digital_ventrio IS NULL OR digital_ventrio = '';
            UPDATE directory_software SET ip = COALESCE(NULLIF(ip,''), json_extract(data_json, '$.IP'), json_extract(data_json, '$.ip')) WHERE ip IS NULL OR ip = '';
            UPDATE directory_software SET usuario = COALESCE(NULLIF(usuario,''), json_extract(data_json, '$.Usuarios'), json_extract(data_json, '$.Usuarios'), json_extract(data_json, '$.usuario')) WHERE usuario IS NULL OR usuario = '';
            "#,
        );
        // Defaults Skymail se vazio
        let _ = conn.execute(
            "INSERT OR IGNORE INTO directory_mail_settings
             (id, skymail_imap_host, skymail_imap_port, skymail_imap_ssl,
              skymail_smtp_host, skymail_smtp_port, skymail_smtp_ssl,
              skymail_pop_host, skymail_pop_port, skymail_pop_ssl, updated_at)
             VALUES ('default', 'imap.skymail.net.br', 993, 1,
                     'smtp.skymail.net.br', 465, 1,
                     'pop.skymail.net.br', 995, 1, datetime('now'))",
            [],
        );

        self.dedupe_machines_by_fingerprint(&conn)?;
        Ok(())
    }

    fn dedupe_machines_by_fingerprint(&self, conn: &Connection) -> Result<(), DbError> {
        let mut stmt = conn.prepare(
            "SELECT id, machine_uuid, serial, mac_primary, hostname, agent_token, last_seen
             FROM machines ORDER BY last_seen DESC",
        )?;
        let rows: Vec<(
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            String,
            String,
            String,
        )> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            })?
            .filter_map(Result::ok)
            .collect();

        let mut seen_fp = std::collections::HashMap::new();
        for (id, uuid, serial, mac, hostname, _token, _last_seen) in rows {
            let fp = belarc_shared::machine_fingerprint(
                uuid.as_deref(),
                serial.as_deref(),
                mac.as_deref(),
                &hostname,
            );
            if let Some(keep_id) = seen_fp.get(&fp) {
                conn.execute(
                    "UPDATE inventory_snapshots SET machine_id=?1 WHERE machine_id=?2",
                    params![keep_id, id],
                )?;
                conn.execute(
                    "UPDATE heartbeats SET machine_id=?1 WHERE machine_id=?2",
                    params![keep_id, id],
                )?;
                conn.execute(
                    "UPDATE alerts SET machine_id=?1 WHERE machine_id=?2",
                    params![keep_id, id],
                )?;
                conn.execute(
                    "UPDATE incident_events SET machine_id=?1 WHERE machine_id=?2",
                    params![keep_id, id],
                )?;
                conn.execute("DELETE FROM machines WHERE id=?1", [&id])?;
            } else {
                seen_fp.insert(fp.clone(), id.clone());
                conn.execute(
                    "UPDATE machines SET machine_fingerprint=?1 WHERE id=?2",
                    params![fp, id],
                )?;
            }
        }
        Ok(())
    }

    pub fn reset_all_data(&self) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "DELETE FROM inventory_deltas;
             DELETE FROM inventory_snapshots;
             DELETE FROM heartbeats;
             DELETE FROM alerts;
             DELETE FROM incident_events;
             DELETE FROM ticket_checklist;
             DELETE FROM ticket_comments;
             DELETE FROM ticket_attachments;
             DELETE FROM tickets;
             DELETE FROM machine_admin;
             DELETE FROM machines;
             DELETE FROM agent_tokens;",
        )?;
        Ok(())
    }

    fn lookup_machine_id(&self, token: &str) -> Result<String, DbError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id FROM machines WHERE agent_token = ?1",
            [token],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(DbError::InvalidToken)
    }

    pub fn machine_id_for_agent_token(&self, token: &str) -> Result<String, DbError> {
        self.lookup_machine_id(token)
    }

    pub fn next_ticket_code(&self) -> Result<String, DbError> {
        let conn = self.conn.lock().unwrap();
        let day = Utc::now().format("%Y%m%d").to_string();
        let prefix = format!("CHM-{day}-");
        // MAX numeric suffix among CHM-YYYYMMDD-NNN codes (ignore PEND-* and non-numeric).
        let mut stmt = conn
            .prepare("SELECT code FROM tickets WHERE code LIKE ?1 AND instr(code, '-PEND-') = 0")?;
        let rows = stmt.query_map([format!("{prefix}%")], |r| r.get::<_, String>(0))?;
        let mut max_n: i64 = 0;
        for code in rows.filter_map(Result::ok) {
            if let Some(suffix) = code.strip_prefix(&prefix) {
                // Only pure numeric NNN (skip PEND-uuid etc.)
                if suffix.chars().all(|c| c.is_ascii_digit()) {
                    if let Ok(n) = suffix.parse::<i64>() {
                        if n > max_n {
                            max_n = n;
                        }
                    }
                }
            }
        }
        Ok(format!("{prefix}{:03}", max_n + 1))
    }

    pub fn create_ticket(
        &self,
        machine_id: &str,
        hostname_snapshot: Option<&str>,
        owner_name_snapshot: Option<&str>,
        title: &str,
        description: Option<&str>,
        priority: &str,
        created_by: &str,
        department: &str,
        requester_user_id: Option<&str>,
    ) -> Result<crate::tickets::Ticket, DbError> {
        let _ = self.get_machine_by_id(machine_id)?;
        let owner = owner_name_snapshot
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let now = Utc::now().to_rfc3339();
        let mut last_err: Option<DbError> = None;
        for _attempt in 0..8 {
            let id = uuid::Uuid::new_v4().to_string();
            let code = self.next_ticket_code()?;
            let conn = self.conn.lock().unwrap();
            match conn.execute(
                "INSERT INTO tickets (id, code, machine_id, hostname_snapshot, owner_name_snapshot,
                 title, description, status, priority, department, requester_user_id, created_by, assignee, nas_path, created_at, updated_at,
                 closed_at, rating, rating_comment)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,'open',?8,?9,?10,?11,NULL,NULL,?12,?12,NULL,NULL,NULL)",
                params![
                    id,
                    code,
                    machine_id,
                    hostname_snapshot,
                    owner,
                    title,
                    description,
                    priority,
                    department,
                    requester_user_id,
                    created_by,
                    now,
                ],
            ) {
                Ok(_) => {
                    drop(conn);
                    return self.get_ticket_by_id(&id);
                }
                Err(rusqlite::Error::SqliteFailure(info, _))
                    if info.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    // UNIQUE on code — retry with next MAX+1
                    last_err = Some(DbError::Sqlite(rusqlite::Error::SqliteFailure(info, None)));
                    continue;
                }
                Err(e) => return Err(DbError::Sqlite(e)),
            }
        }
        Err(last_err.unwrap_or_else(|| {
            DbError::InvalidInput("falha ao gerar codigo unico de chamado".into())
        }))
    }

    pub fn set_ticket_nas_path(&self, id: &str, nas_path: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE tickets SET nas_path=?1, updated_at=?2 WHERE id=?3",
            params![nas_path, now, id],
        )?;
        Ok(())
    }

    fn hydrate_ticket(
        &self,
        mut ticket: crate::tickets::Ticket,
    ) -> Result<crate::tickets::Ticket, DbError> {
        ticket.attachments = self.list_ticket_attachments(&ticket.id)?;
        ticket.comments = self.list_ticket_comments(&ticket.id)?;
        ticket.checklist = self.list_ticket_checklist(&ticket.id)?;
        Ok(ticket)
    }

    pub fn get_ticket_by_id(&self, id: &str) -> Result<crate::tickets::Ticket, DbError> {
        let conn = self.conn.lock().unwrap();
        let ticket = conn
            .query_row(
                "SELECT id, code, machine_id, hostname_snapshot, owner_name_snapshot, title, description, status,
                 priority, department, requester_user_id, created_by, assignee, nas_path, created_at, updated_at,
                 closed_at, rating, rating_comment, reopen_pending, reopen_reason, reopen_requested_at,
                 last_reopen_reason, resolution
                 FROM tickets WHERE id=?1",
                [id],
                parse_ticket_row,
            )
            .optional()?
            .ok_or(DbError::NotFound)?;
        drop(conn);
        self.hydrate_ticket(ticket)
    }

    pub fn get_ticket_by_code(&self, code: &str) -> Result<crate::tickets::Ticket, DbError> {
        let conn = self.conn.lock().unwrap();
        let id: String = conn
            .query_row("SELECT id FROM tickets WHERE code=?1", [code], |r| r.get(0))
            .optional()?
            .ok_or(DbError::NotFound)?;
        drop(conn);
        self.get_ticket_by_id(&id)
    }

    pub fn find_ticket_by_code(
        &self,
        code: &str,
    ) -> Result<Option<crate::tickets::Ticket>, DbError> {
        match self.get_ticket_by_code(code) {
            Ok(t) => Ok(Some(t)),
            Err(DbError::NotFound) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Garante máquina existente para import de chamado (stub se ausente).
    pub fn ensure_machine_stub(
        &self,
        machine_id: &str,
        hostname: Option<&str>,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        let exists: bool = conn
            .query_row("SELECT 1 FROM machines WHERE id=?1", [machine_id], |_| {
                Ok(true)
            })
            .optional()?
            .unwrap_or(false);
        if exists {
            return Ok(());
        }
        let now = Utc::now().to_rfc3339();
        let host = hostname
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("DESCONHECIDO");
        let token = format!("nas-import-{}", machine_id);
        conn.execute(
            "INSERT INTO machines (id, agent_token, hostname, serial, machine_uuid, mac_primary,
             machine_fingerprint, status, first_seen, last_seen)
             VALUES (?1,?2,?3,NULL,NULL,NULL,?4,'offline',?5,?5)",
            params![
                machine_id,
                token,
                host,
                format!("nas-stub:{machine_id}"),
                now
            ],
        )?;
        Ok(())
    }

    /// Insere chamado a partir do meta.json do NAS (preserva code).
    pub fn import_ticket_from_nas(
        &self,
        meta: &crate::tickets::NasTicketMeta,
        nas_dir: &Path,
    ) -> Result<crate::tickets::Ticket, DbError> {
        self.ensure_machine_stub(&meta.machine_id, meta.hostname_snapshot.as_deref())?;
        let id = uuid::Uuid::new_v4().to_string();
        let status =
            crate::tickets::normalize_status(&meta.status).unwrap_or(crate::tickets::STATUS_OPEN);
        let priority = crate::tickets::normalize_priority(&meta.priority).unwrap_or("normal");
        let updated = meta
            .updated_at
            .clone()
            .unwrap_or_else(|| meta.created_at.clone());
        let nas_path = nas_dir.to_string_lossy().to_string();
        let reopen = if meta.reopen_pending { 1 } else { 0 };
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO tickets (id, code, machine_id, hostname_snapshot, owner_name_snapshot,
             title, description, status, priority, created_by, assignee, nas_path, created_at, updated_at,
             closed_at, rating, rating_comment, reopen_pending, reopen_reason, reopen_requested_at,
             last_reopen_reason, resolution)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22)",
            params![
                id,
                meta.code,
                meta.machine_id,
                meta.hostname_snapshot,
                meta.owner_name_snapshot,
                meta.title,
                meta.description,
                status,
                priority,
                meta.created_by,
                meta.assignee,
                nas_path,
                meta.created_at,
                updated,
                meta.closed_at,
                meta.rating,
                meta.rating_comment,
                reopen,
                meta.reopen_reason,
                meta.reopen_requested_at,
                meta.last_reopen_reason,
                meta.resolution,
            ],
        )?;
        drop(conn);
        self.get_ticket_by_id(&id)
    }

    /// Atualiza chamado existente com campos do NAS (NAS vence).
    pub fn apply_nas_ticket_meta(
        &self,
        id: &str,
        meta: &crate::tickets::NasTicketMeta,
        nas_dir: &Path,
    ) -> Result<crate::tickets::Ticket, DbError> {
        let _ = self.get_ticket_by_id(id)?;
        let status =
            crate::tickets::normalize_status(&meta.status).unwrap_or(crate::tickets::STATUS_OPEN);
        let priority = crate::tickets::normalize_priority(&meta.priority).unwrap_or("normal");
        let updated = meta
            .updated_at
            .clone()
            .unwrap_or_else(|| meta.created_at.clone());
        let nas_path = nas_dir.to_string_lossy().to_string();
        let reopen = if meta.reopen_pending { 1 } else { 0 };
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE tickets SET hostname_snapshot=COALESCE(?1, hostname_snapshot),
             owner_name_snapshot=COALESCE(?2, owner_name_snapshot),
             title=?3, description=?4, status=?5, priority=?6,
             created_by=COALESCE(?7, created_by), assignee=?8, nas_path=?9,
             created_at=?10, updated_at=?11, closed_at=?12, rating=?13, rating_comment=?14,
             reopen_pending=?15, reopen_reason=?16, reopen_requested_at=?17,
             last_reopen_reason=?18, resolution=?19 WHERE id=?20",
            params![
                meta.hostname_snapshot,
                meta.owner_name_snapshot,
                meta.title,
                meta.description,
                status,
                priority,
                meta.created_by,
                meta.assignee,
                nas_path,
                meta.created_at,
                updated,
                meta.closed_at,
                meta.rating,
                meta.rating_comment,
                reopen,
                meta.reopen_reason,
                meta.reopen_requested_at,
                meta.last_reopen_reason,
                meta.resolution,
                id,
            ],
        )?;
        drop(conn);
        self.get_ticket_by_id(id)
    }

    pub fn list_tickets(
        &self,
        status: Option<&str>,
        machine_id: Option<&str>,
    ) -> Result<Vec<crate::tickets::Ticket>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut out: Vec<crate::tickets::Ticket> = match (status, machine_id) {
            (Some(s), Some(m)) => {
                let mut stmt = conn.prepare(
                    "SELECT id, code, machine_id, hostname_snapshot, owner_name_snapshot, title, description, status,
                     priority, department, requester_user_id, created_by, assignee, nas_path, created_at, updated_at,
                     closed_at, rating, rating_comment, reopen_pending, reopen_reason, reopen_requested_at,
                     last_reopen_reason, resolution
                     FROM tickets WHERE status=?1 AND machine_id=?2
                     ORDER BY updated_at DESC LIMIT 500",
                )?;
                let rows = stmt.query_map(params![s, m], parse_ticket_row)?;
                rows.filter_map(Result::ok).collect()
            }
            (Some(s), None) => {
                let mut stmt = conn.prepare(
                    "SELECT id, code, machine_id, hostname_snapshot, owner_name_snapshot, title, description, status,
                     priority, department, requester_user_id, created_by, assignee, nas_path, created_at, updated_at,
                     closed_at, rating, rating_comment, reopen_pending, reopen_reason, reopen_requested_at,
                     last_reopen_reason, resolution
                     FROM tickets WHERE status=?1
                     ORDER BY updated_at DESC LIMIT 500",
                )?;
                let rows = stmt.query_map(params![s], parse_ticket_row)?;
                rows.filter_map(Result::ok).collect()
            }
            (None, Some(m)) => {
                let mut stmt = conn.prepare(
                    "SELECT id, code, machine_id, hostname_snapshot, owner_name_snapshot, title, description, status,
                     priority, department, requester_user_id, created_by, assignee, nas_path, created_at, updated_at,
                     closed_at, rating, rating_comment, reopen_pending, reopen_reason, reopen_requested_at,
                     last_reopen_reason, resolution
                     FROM tickets WHERE machine_id=?1
                     ORDER BY updated_at DESC LIMIT 500",
                )?;
                let rows = stmt.query_map(params![m], parse_ticket_row)?;
                rows.filter_map(Result::ok).collect()
            }
            (None, None) => {
                let mut stmt = conn.prepare(
                    "SELECT id, code, machine_id, hostname_snapshot, owner_name_snapshot, title, description, status,
                     priority, department, requester_user_id, created_by, assignee, nas_path, created_at, updated_at,
                     closed_at, rating, rating_comment, reopen_pending, reopen_reason, reopen_requested_at,
                     last_reopen_reason, resolution
                     FROM tickets ORDER BY updated_at DESC LIMIT 500",
                )?;
                let rows = stmt.query_map([], parse_ticket_row)?;
                rows.filter_map(Result::ok).collect()
            }
        };
        drop(conn);
        for t in &mut out {
            *t = self.hydrate_ticket(t.clone())?;
        }
        Ok(out)
    }

    pub fn list_tickets_for_department(
        &self,
        department: &str,
    ) -> Result<Vec<crate::tickets::Ticket>, DbError> {
        self.list_tickets_filtered("department", department)
    }

    pub fn list_tickets_for_requester(
        &self,
        user_id: &str,
    ) -> Result<Vec<crate::tickets::Ticket>, DbError> {
        self.list_tickets_filtered("requester_user_id", user_id)
    }

    pub fn list_tickets_for_departments(
        &self,
        departments: &[String],
    ) -> Result<Vec<crate::tickets::Ticket>, DbError> {
        let mut out = Vec::new();
        for department in departments {
            out.extend(self.list_tickets_for_department(department)?);
        }
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        out.dedup_by(|a, b| a.id == b.id);
        Ok(out)
    }

    fn list_tickets_filtered(
        &self,
        column: &str,
        value: &str,
    ) -> Result<Vec<crate::tickets::Ticket>, DbError> {
        let query = match column {
            "department" => "SELECT id, code, machine_id, hostname_snapshot, owner_name_snapshot, title, description, status, priority, department, requester_user_id, created_by, assignee, nas_path, created_at, updated_at, closed_at, rating, rating_comment, reopen_pending, reopen_reason, reopen_requested_at, last_reopen_reason, resolution FROM tickets WHERE department=?1 ORDER BY updated_at DESC LIMIT 500",
            "requester_user_id" => "SELECT id, code, machine_id, hostname_snapshot, owner_name_snapshot, title, description, status, priority, department, requester_user_id, created_by, assignee, nas_path, created_at, updated_at, closed_at, rating, rating_comment, reopen_pending, reopen_reason, reopen_requested_at, last_reopen_reason, resolution FROM tickets WHERE requester_user_id=?1 ORDER BY updated_at DESC LIMIT 500",
            _ => return Err(DbError::InvalidInput("filtro de chamados invalido".into())),
        };
        let mut out: Vec<_> = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn.prepare(query)?;
            let rows = stmt.query_map([value], parse_ticket_row)?;
            rows.filter_map(Result::ok).collect()
        };
        for ticket in &mut out {
            *ticket = self.hydrate_ticket(ticket.clone())?;
        }
        Ok(out)
    }

    pub fn set_ticket_department(
        &self,
        id: &str,
        department: &str,
    ) -> Result<crate::tickets::Ticket, DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE tickets SET department=?1, updated_at=?2 WHERE id=?3",
            params![department, Utc::now().to_rfc3339(), id],
        )?;
        drop(conn);
        self.get_ticket_by_id(id)
    }

    pub fn delete_ticket(&self, id: &str) -> Result<crate::tickets::Ticket, DbError> {
        let ticket = self
            .get_ticket_by_id(id)
            .or_else(|_| self.get_ticket_by_code(id))?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM ticket_checklist WHERE ticket_id=?1",
            [&ticket.id],
        )?;
        conn.execute(
            "DELETE FROM ticket_comments WHERE ticket_id=?1",
            [&ticket.id],
        )?;
        conn.execute(
            "DELETE FROM ticket_attachments WHERE ticket_id=?1",
            [&ticket.id],
        )?;
        let n = conn.execute("DELETE FROM tickets WHERE id=?1", [&ticket.id])?;
        if n == 0 {
            return Err(DbError::NotFound);
        }
        Ok(ticket)
    }

    pub fn update_ticket(
        &self,
        id: &str,
        title: Option<&str>,
        description: Option<&str>,
        status: Option<&str>,
        priority: Option<&str>,
        assignee: Option<&str>,
    ) -> Result<crate::tickets::Ticket, DbError> {
        let existing = self.get_ticket_by_id(id)?;
        let now = Utc::now().to_rfc3339();
        let closing = status == Some(crate::tickets::STATUS_DONE) && existing.closed_at.is_none();
        let reopening = matches!(status, Some(s) if s != crate::tickets::STATUS_DONE)
            && existing.status == crate::tickets::STATUS_DONE;
        let conn = self.conn.lock().unwrap();
        if closing {
            conn.execute(
                "UPDATE tickets SET title=COALESCE(?1, title), description=COALESCE(?2, description),
                 status=COALESCE(?3, status), priority=COALESCE(?4, priority),
                 assignee=COALESCE(?5, assignee), updated_at=?6, closed_at=?6 WHERE id=?7",
                params![
                    title,
                    description,
                    status,
                    priority,
                    assignee,
                    now,
                    existing.id,
                ],
            )?;
        } else if reopening {
            conn.execute(
                "UPDATE tickets SET title=COALESCE(?1, title), description=COALESCE(?2, description),
                 status=COALESCE(?3, status), priority=COALESCE(?4, priority),
                 assignee=COALESCE(?5, assignee), updated_at=?6, closed_at=NULL,
                 reopen_pending=0, reopen_reason=NULL, reopen_requested_at=NULL,
                 resolution=NULL WHERE id=?7",
                params![
                    title,
                    description,
                    status,
                    priority,
                    assignee,
                    now,
                    existing.id,
                ],
            )?;
        } else {
            conn.execute(
                "UPDATE tickets SET title=COALESCE(?1, title), description=COALESCE(?2, description),
                 status=COALESCE(?3, status), priority=COALESCE(?4, priority),
                 assignee=COALESCE(?5, assignee), updated_at=?6 WHERE id=?7",
                params![
                    title,
                    description,
                    status,
                    priority,
                    assignee,
                    now,
                    existing.id,
                ],
            )?;
        }
        drop(conn);
        self.get_ticket_by_id(id)
    }

    pub fn close_ticket(
        &self,
        id: &str,
        resolution: Option<&str>,
    ) -> Result<crate::tickets::Ticket, DbError> {
        let existing = self.get_ticket_by_id(id)?;
        if existing.status == crate::tickets::STATUS_DONE {
            return Err(DbError::InvalidInput("chamado ja esta concluido".into()));
        }
        let now = Utc::now().to_rfc3339();
        let res = resolution.map(str::trim).filter(|s| !s.is_empty());
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE tickets SET status=?1, updated_at=?2, closed_at=?2, resolution=?3,
             reopen_pending=0, reopen_reason=NULL, reopen_requested_at=NULL WHERE id=?4",
            params![crate::tickets::STATUS_DONE, now, res, existing.id],
        )?;
        drop(conn);
        self.get_ticket_by_id(id)
    }

    pub fn set_last_reopen_reason(&self, id: &str, reason: &str) -> Result<(), DbError> {
        let reason = reason.trim();
        if reason.is_empty() {
            return Ok(());
        }
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE tickets SET last_reopen_reason=?1, updated_at=?2 WHERE id=?3",
            params![reason, now, id],
        )?;
        Ok(())
    }

    pub fn rate_ticket(
        &self,
        id: &str,
        rating: i32,
        comment: Option<&str>,
    ) -> Result<crate::tickets::Ticket, DbError> {
        let existing = self.get_ticket_by_id(id)?;
        if existing.status != crate::tickets::STATUS_DONE {
            return Err(DbError::InvalidInput(
                "so e possivel avaliar chamado fechado".into(),
            ));
        }
        if !(1..=5).contains(&rating) {
            return Err(DbError::InvalidInput("rating deve ser 1 a 5".into()));
        }
        if existing.rating.is_some() {
            return Err(DbError::InvalidInput("chamado ja avaliado".into()));
        }
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE tickets SET rating=?1, rating_comment=?2, updated_at=?3 WHERE id=?4",
            params![rating, comment, now, existing.id],
        )?;
        drop(conn);
        self.get_ticket_by_id(id)
    }

    pub fn set_reopen_request(
        &self,
        id: &str,
        reason: &str,
    ) -> Result<crate::tickets::Ticket, DbError> {
        let reason = reason.trim();
        if reason.is_empty() {
            return Err(DbError::InvalidInput("motivo obrigatorio".into()));
        }
        let existing = self.get_ticket_by_id(id)?;
        if existing.status != crate::tickets::STATUS_DONE {
            return Err(DbError::InvalidInput(
                "so e possivel pedir reabertura de chamado concluido".into(),
            ));
        }
        if existing.reopen_pending {
            return Err(DbError::InvalidInput(
                "ja existe pedido de reabertura pendente".into(),
            ));
        }
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE tickets SET reopen_pending=1, reopen_reason=?1, reopen_requested_at=?2, updated_at=?2
             WHERE id=?3",
            params![reason, now, existing.id],
        )?;
        drop(conn);
        self.get_ticket_by_id(id)
    }

    pub fn clear_reopen_request(&self, id: &str) -> Result<(), DbError> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE tickets SET reopen_pending=0, reopen_reason=NULL, reopen_requested_at=NULL, updated_at=?1
             WHERE id=?2",
            params![now, id],
        )?;
        Ok(())
    }

    pub fn list_ticket_attachments(
        &self,
        ticket_id: &str,
    ) -> Result<Vec<crate::tickets::TicketAttachment>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, ticket_id, filename, stored_name, created_at
             FROM ticket_attachments WHERE ticket_id=?1 ORDER BY created_at",
        )?;
        let rows = stmt.query_map([ticket_id], |row| {
            Ok(crate::tickets::TicketAttachment {
                id: row.get(0)?,
                ticket_id: row.get(1)?,
                filename: row.get(2)?,
                stored_name: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn get_ticket_attachment(
        &self,
        ticket_id: &str,
        attachment_id: &str,
    ) -> Result<crate::tickets::TicketAttachment, DbError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, ticket_id, filename, stored_name, created_at
             FROM ticket_attachments WHERE id=?1 AND ticket_id=?2",
            params![attachment_id, ticket_id],
            |row| {
                Ok(crate::tickets::TicketAttachment {
                    id: row.get(0)?,
                    ticket_id: row.get(1)?,
                    filename: row.get(2)?,
                    stored_name: row.get(3)?,
                    created_at: row.get(4)?,
                })
            },
        )
        .optional()?
        .ok_or(DbError::NotFound)
    }

    pub fn add_ticket_attachment(
        &self,
        ticket_id: &str,
        filename: &str,
        stored_name: &str,
    ) -> Result<crate::tickets::TicketAttachment, DbError> {
        let _ = self.get_ticket_by_id(ticket_id)?;
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO ticket_attachments (id, ticket_id, filename, stored_name, created_at)
             VALUES (?1,?2,?3,?4,?5)",
            params![id, ticket_id, filename, stored_name, now],
        )?;
        Ok(crate::tickets::TicketAttachment {
            id,
            ticket_id: ticket_id.to_string(),
            filename: filename.to_string(),
            stored_name: stored_name.to_string(),
            created_at: now,
        })
    }

    pub fn list_ticket_comments(
        &self,
        ticket_id: &str,
    ) -> Result<Vec<crate::tickets::TicketComment>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, ticket_id, author_role, author_name, body, created_at
             FROM ticket_comments WHERE ticket_id=?1 ORDER BY created_at",
        )?;
        let rows = stmt.query_map([ticket_id], |row| {
            Ok(crate::tickets::TicketComment {
                id: row.get(0)?,
                ticket_id: row.get(1)?,
                author_role: row.get(2)?,
                author_name: row.get(3)?,
                body: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn add_ticket_comment(
        &self,
        ticket_id: &str,
        author_role: &str,
        author_name: Option<&str>,
        body: &str,
    ) -> Result<crate::tickets::TicketComment, DbError> {
        let body = body.trim();
        if body.is_empty() {
            return Err(DbError::InvalidInput("comentario vazio".into()));
        }
        let _ = self.get_ticket_by_id(ticket_id)?;
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO ticket_comments (id, ticket_id, author_role, author_name, body, created_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![id, ticket_id, author_role, author_name, body, now],
        )?;
        conn.execute(
            "UPDATE tickets SET updated_at=?1 WHERE id=?2",
            params![now, ticket_id],
        )?;
        Ok(crate::tickets::TicketComment {
            id,
            ticket_id: ticket_id.to_string(),
            author_role: author_role.to_string(),
            author_name: author_name.map(|s| s.to_string()),
            body: body.to_string(),
            created_at: now,
        })
    }

    pub fn list_ticket_checklist(
        &self,
        ticket_id: &str,
    ) -> Result<Vec<crate::tickets::ChecklistItem>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, ticket_id, label, done, sort_order, created_at, updated_at
             FROM ticket_checklist WHERE ticket_id=?1 ORDER BY sort_order, created_at",
        )?;
        let rows = stmt.query_map([ticket_id], |row| {
            let done_i: i64 = row.get(3)?;
            Ok(crate::tickets::ChecklistItem {
                id: row.get(0)?,
                ticket_id: row.get(1)?,
                label: row.get(2)?,
                done: done_i != 0,
                sort_order: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn add_checklist_item(
        &self,
        ticket_id: &str,
        label: &str,
    ) -> Result<crate::tickets::ChecklistItem, DbError> {
        let label = label.trim();
        if label.is_empty() {
            return Err(DbError::InvalidInput("item vazio".into()));
        }
        let _ = self.get_ticket_by_id(ticket_id)?;
        let conn = self.conn.lock().unwrap();
        let max_order: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(sort_order), -1) FROM ticket_checklist WHERE ticket_id=?1",
                [ticket_id],
                |r| r.get(0),
            )
            .unwrap_or(-1);
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let sort_order = (max_order + 1) as i32;
        conn.execute(
            "INSERT INTO ticket_checklist (id, ticket_id, label, done, sort_order, created_at, updated_at)
             VALUES (?1,?2,?3,0,?4,?5,?5)",
            params![id, ticket_id, label, sort_order, now],
        )?;
        Ok(crate::tickets::ChecklistItem {
            id,
            ticket_id: ticket_id.to_string(),
            label: label.to_string(),
            done: false,
            sort_order,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn update_checklist_item(
        &self,
        ticket_id: &str,
        item_id: &str,
        done: Option<bool>,
        label: Option<&str>,
    ) -> Result<crate::tickets::ChecklistItem, DbError> {
        let items = self.list_ticket_checklist(ticket_id)?;
        let existing = items
            .into_iter()
            .find(|i| i.id == item_id)
            .ok_or(DbError::NotFound)?;
        let now = Utc::now().to_rfc3339();
        let new_done = done.unwrap_or(existing.done);
        let new_label = label
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&existing.label);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE ticket_checklist SET done=?1, label=?2, updated_at=?3 WHERE id=?4 AND ticket_id=?5",
            params![if new_done { 1 } else { 0 }, new_label, now, item_id, ticket_id],
        )?;
        Ok(crate::tickets::ChecklistItem {
            id: existing.id,
            ticket_id: existing.ticket_id,
            label: new_label.to_string(),
            done: new_done,
            sort_order: existing.sort_order,
            created_at: existing.created_at,
            updated_at: now,
        })
    }

    pub fn delete_checklist_item(&self, ticket_id: &str, item_id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "DELETE FROM ticket_checklist WHERE id=?1 AND ticket_id=?2",
            params![item_id, ticket_id],
        )?;
        if n == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    pub fn register_agent(&self, payload: &RegisterPayload) -> Result<String, DbError> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let fingerprint = belarc_shared::machine_fingerprint(
            payload.machine_uuid.as_deref(),
            payload.serial.as_deref(),
            payload.mac_primary.as_deref(),
            &payload.hostname,
        );

        // Mesmo PC (fingerprint) com token novo -> reutiliza registro existente
        if let Some(id) = conn
            .query_row(
                "SELECT id FROM machines WHERE machine_fingerprint = ?1",
                [&fingerprint],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            conn.execute(
                "UPDATE machines SET agent_token=?1, hostname=?2, serial=?3, machine_uuid=?4,
                 mac_primary=?5, last_seen=?6, status='online' WHERE id=?7",
                params![
                    payload.agent_token,
                    payload.hostname,
                    payload.serial,
                    payload.machine_uuid,
                    payload.mac_primary,
                    now,
                    id,
                ],
            )?;
            return Ok(id);
        }

        if let Some(id) = conn
            .query_row(
                "SELECT id FROM machines WHERE agent_token = ?1",
                [&payload.agent_token],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            conn.execute(
                "UPDATE machines SET hostname=?1, serial=?2, machine_uuid=?3, mac_primary=?4,
                 machine_fingerprint=?5, last_seen=?6, status='online' WHERE id=?7",
                params![
                    payload.hostname,
                    payload.serial,
                    payload.machine_uuid,
                    payload.mac_primary,
                    fingerprint,
                    now,
                    id,
                ],
            )?;
            return Ok(id);
        }

        // Um agente novo só pode entrar com uma matrícula criada por uma sessão TI.
        // Depois do primeiro uso, o mesmo token continua ligado exclusivamente à
        // máquina registrada acima, mas não pode cadastrar outro computador.
        let enrollment_is_valid = conn
            .query_row(
                "SELECT 1 FROM agent_tokens
                 WHERE token = ?1 AND active = 1 AND expires_at > ?2 AND consumed_at IS NULL",
                params![payload.agent_token, now],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !enrollment_is_valid {
            return Err(DbError::InvalidToken);
        }

        let id = belarc_shared::new_id();
        conn.execute(
            "INSERT INTO machines (id, agent_token, hostname, serial, machine_uuid, mac_primary, machine_fingerprint, status, first_seen, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'online', ?8, ?8)",
            params![
                id,
                payload.agent_token,
                payload.hostname,
                payload.serial,
                payload.machine_uuid,
                payload.mac_primary,
                fingerprint,
                now,
            ],
        )?;
        conn.execute(
            "UPDATE agent_tokens SET active = 0, consumed_at = ?2 WHERE token = ?1",
            params![payload.agent_token, now],
        )?;
        Ok(id)
    }

    pub fn process_heartbeat(&self, payload: &HeartbeatPayload) -> Result<(), DbError> {
        let machine_id = self.lookup_machine_id(&payload.agent_token)?;
        let now = Utc::now();
        let conn = self.conn.lock().unwrap();

        conn.execute(
            "UPDATE machines SET hostname=?1, serial=COALESCE(?2, serial), machine_uuid=COALESCE(?3, machine_uuid),
             mac_primary=COALESCE(?4, mac_primary), status='online', logged_user=?5, ip_address=?6,
             uptime_seconds=?7, last_boot=?8, last_seen=?9 WHERE id=?10",
            params![
                payload.hostname,
                payload.serial,
                payload.uuid,
                payload.mac_primary,
                payload.logged_user,
                payload.ip_address,
                payload.uptime_seconds,
                payload.last_boot.map(|d| d.to_rfc3339()),
                now.to_rfc3339(),
                machine_id,
            ],
        )?;

        conn.execute(
            "INSERT INTO heartbeats (machine_id, logged_user, ip_address, uptime_seconds, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                machine_id,
                payload.logged_user,
                payload.ip_address,
                payload.uptime_seconds,
                now.to_rfc3339(),
            ],
        )?;

        Ok(())
    }

    pub fn process_inventory(
        &self,
        payload: &InventoryPayload,
    ) -> Result<Vec<(String, String)>, DbError> {
        let machine_id = self.lookup_machine_id(&payload.agent_token)?;
        let conn = self.conn.lock().unwrap();
        let now = payload.collected_at.to_rfc3339();
        let mut changed = Vec::new();

        for collector in &payload.collectors {
            if collector.error.is_some() {
                continue;
            }

            let prev_hash: Option<String> = conn
                .query_row(
                    "SELECT data_hash FROM inventory_snapshots WHERE machine_id=?1 AND collector_name=?2",
                    params![machine_id, collector.name],
                    |row| row.get(0),
                )
                .optional()?;

            if prev_hash.as_deref() == Some(collector.hash.as_str()) {
                continue;
            }

            if let Some(ref old_hash) = prev_hash {
                changed.push((collector.name.clone(), old_hash.clone()));
            }

            let data_json = serde_json::to_string(&collector.data).unwrap_or_else(|_| "{}".into());

            conn.execute(
                "INSERT INTO inventory_snapshots (machine_id, tier, collector_name, collector_version, data_json, data_hash, collected_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(machine_id, collector_name) DO UPDATE SET
                 tier=excluded.tier, collector_version=excluded.collector_version,
                 data_json=excluded.data_json, data_hash=excluded.data_hash, collected_at=excluded.collected_at",
                params![
                    machine_id,
                    format!("{:?}", payload.tier).to_lowercase(),
                    collector.name,
                    collector.version,
                    data_json,
                    collector.hash,
                    now,
                ],
            )?;

            if prev_hash.is_some() {
                conn.execute(
                    "INSERT INTO inventory_deltas (machine_id, collector_name, field_path, old_value, new_value, created_at)
                     VALUES (?1, ?2, '*', 'changed', ?3, ?4)",
                    params![machine_id, collector.name, collector.hash, now],
                )?;
            }
        }

        if let Some(identity) = payload.collectors.iter().find(|c| c.name == "identity") {
            let logged_user = identity.data.get("logged_user").and_then(|v| v.as_str());
            let ip_address = identity
                .data
                .get("ip_primary")
                .and_then(|v| v.as_str())
                .or_else(|| identity.data.get("ip_address").and_then(|v| v.as_str()));
            let uptime = identity.data.get("uptime_seconds").and_then(|v| v.as_u64());
            conn.execute(
                "UPDATE machines SET logged_user=COALESCE(?1, logged_user), ip_address=COALESCE(?2, ip_address),
                 uptime_seconds=COALESCE(?3, uptime_seconds), last_seen=?4, status='online' WHERE id=?5",
                params![logged_user, ip_address, uptime, Utc::now().to_rfc3339(), machine_id],
            )?;
        } else {
            conn.execute(
                "UPDATE machines SET last_seen=?1, status='online' WHERE id=?2",
                params![Utc::now().to_rfc3339(), machine_id],
            )?;
        }

        Ok(changed)
    }

    pub fn update_offline_machines(&self, threshold_minutes: u64) -> Result<u64, DbError> {
        let cutoff = (Utc::now() - Duration::minutes(threshold_minutes as i64)).to_rfc3339();
        let conn = self.conn.lock().unwrap();
        let updated = conn.execute(
            "UPDATE machines SET status='offline' WHERE status='online' AND last_seen < ?1",
            [cutoff],
        )?;
        Ok(updated as u64)
    }

    pub fn list_machines(&self) -> Result<Vec<MachineSummary>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, hostname, serial, status, logged_user, ip_address, uptime_seconds, last_seen, first_seen, health_score
             FROM machines ORDER BY hostname",
        )?;
        let rows = stmt.query_map([], |row| {
            let status_str: String = row.get(3)?;
            let status = if status_str == "online" {
                MachineStatus::Online
            } else {
                MachineStatus::Offline
            };
            Ok(MachineSummary {
                id: row.get(0)?,
                hostname: row.get(1)?,
                serial: row.get(2)?,
                status,
                logged_user: row.get(4)?,
                ip_address: row.get(5)?,
                uptime_seconds: row.get(6)?,
                last_seen: DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                first_seen: DateTime::parse_from_rfc3339(&row.get::<_, String>(8)?)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                health_score: row.get(9)?,
            })
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn get_machine_collectors(
        &self,
        machine_id: &str,
    ) -> Result<Vec<CollectorResult>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT collector_name, collector_version, data_json, data_hash FROM inventory_snapshots WHERE machine_id=?1",
        )?;
        let rows = stmt.query_map([machine_id], |row| {
            let data_json: String = row.get(2)?;
            let data: serde_json::Value =
                serde_json::from_str(&data_json).unwrap_or(serde_json::json!({}));
            Ok(CollectorResult {
                name: row.get(0)?,
                version: row.get(1)?,
                data,
                hash: row.get(3)?,
                duration_ms: 0,
                error: None,
            })
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn get_machine_by_id(&self, id: &str) -> Result<MachineSummary, DbError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, hostname, serial, status, logged_user, ip_address, uptime_seconds, last_seen, first_seen, health_score
             FROM machines WHERE id=?1",
            [id],
            |row| {
                let status_str: String = row.get(3)?;
                Ok(MachineSummary {
                    id: row.get(0)?,
                    hostname: row.get(1)?,
                    serial: row.get(2)?,
                    status: if status_str == "online" {
                        MachineStatus::Online
                    } else {
                        MachineStatus::Offline
                    },
                    logged_user: row.get(4)?,
                    ip_address: row.get(5)?,
                    uptime_seconds: row.get(6)?,
                    last_seen: DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                        .map(|d| d.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    first_seen: DateTime::parse_from_rfc3339(&row.get::<_, String>(8)?)
                        .map(|d| d.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    health_score: row.get(9)?,
                })
            },
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => DbError::NotFound,
            other => DbError::Sqlite(other),
        })
    }

    pub fn machine_id_by_token(&self, token: &str) -> Result<String, DbError> {
        self.lookup_machine_id(token)
    }

    pub fn get_machine_admin(
        &self,
        machine_id: &str,
    ) -> Result<crate::admin::MachineAdmin, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut admin = conn
            .query_row(
                "SELECT owner_name, ramal, primary_email, network_cable, cybersul_user,
                    nas_user, notes, maintenance_status, maintenance_notes, ti_comments,
                    ticket_default_department, ticket_attendant_department, updated_at
             FROM machine_admin WHERE machine_id=?1",
                [machine_id],
                |row| {
                    let updated: Option<String> = row.get(12)?;
                    Ok(crate::admin::MachineAdmin {
                        owner_name: row.get(0)?,
                        ramal: row.get(1)?,
                        primary_email: row.get(2)?,
                        network_cable: row.get(3)?,
                        cybersul_user: row.get(4)?,
                        nas_user: row.get(5)?,
                        notes: row.get(6)?,
                        maintenance_status: row.get(7)?,
                        maintenance_notes: row.get(8)?,
                        ti_comments: row.get(9)?,
                        ticket_default_department: row.get(10)?,
                        ticket_attendant_department: row.get(11)?,
                        ticket_receive_departments: Vec::new(),
                        updated_at: updated
                            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                            .map(|d| d.with_timezone(&Utc)),
                    })
                },
            )
            .optional()?
            .unwrap_or_default();
        drop(conn);
        admin.ticket_receive_departments = self.list_machine_ticket_departments(machine_id)?;
        Ok(admin)
    }

    pub fn upsert_machine_admin(
        &self,
        machine_id: &str,
        admin: &crate::admin::MachineAdmin,
    ) -> Result<crate::admin::MachineAdmin, DbError> {
        let _ = self.get_machine_by_id(machine_id)?;
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO machine_admin (machine_id, owner_name, ramal, primary_email, network_cable,
             cybersul_user, nas_user, notes,
             maintenance_status, maintenance_notes, ti_comments, ticket_default_department, ticket_attendant_department, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
             ON CONFLICT(machine_id) DO UPDATE SET
             owner_name=excluded.owner_name, ramal=excluded.ramal, primary_email=excluded.primary_email,
             network_cable=excluded.network_cable, cybersul_user=excluded.cybersul_user,
             nas_user=excluded.nas_user, notes=excluded.notes,
             maintenance_status=excluded.maintenance_status, maintenance_notes=excluded.maintenance_notes,
             ti_comments=excluded.ti_comments, ticket_default_department=excluded.ticket_default_department,
             ticket_attendant_department=excluded.ticket_attendant_department, updated_at=excluded.updated_at",
            params![
                machine_id,
                admin.owner_name,
                admin.ramal,
                admin.primary_email,
                admin.network_cable,
                admin.cybersul_user,
                admin.nas_user,
                admin.notes,
                admin.maintenance_status,
                admin.maintenance_notes,
                admin.ti_comments,
                admin.ticket_default_department,
                admin.ticket_attendant_department,
                now,
            ],
        )?;
        drop(conn);
        self.set_machine_ticket_departments(machine_id, &admin.ticket_receive_departments)?;
        self.get_machine_admin(machine_id)
    }

    pub fn list_machine_ticket_departments(
        &self,
        machine_id: &str,
    ) -> Result<Vec<String>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt=conn.prepare("SELECT department FROM machine_ticket_departments WHERE machine_id=?1 ORDER BY department")?;
        let rows = stmt.query_map([machine_id], |r| r.get(0))?;
        Ok(rows.filter_map(Result::ok).collect())
    }
    pub fn set_machine_ticket_departments(
        &self,
        machine_id: &str,
        departments: &[String],
    ) -> Result<(), DbError> {
        let _ = self.get_machine_by_id(machine_id)?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM machine_ticket_departments WHERE machine_id=?1",
            [machine_id],
        )?;
        for department in departments {
            conn.execute("INSERT OR IGNORE INTO machine_ticket_departments (machine_id,department) VALUES (?1,?2)",params![machine_id,department])?;
        }
        Ok(())
    }

    pub fn list_ticket_departments(
        &self,
        include_inactive: bool,
    ) -> Result<Vec<TicketDepartment>, DbError> {
        let conn = self.conn.lock().unwrap();
        let sql = if include_inactive {
            "SELECT slug,name,active,system,created_at,updated_at FROM ticket_departments ORDER BY active DESC, name COLLATE NOCASE"
        } else {
            "SELECT slug,name,active,system,created_at,updated_at FROM ticket_departments WHERE active=1 ORDER BY name COLLATE NOCASE"
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([], |row| {
            Ok(TicketDepartment {
                slug: row.get(0)?,
                name: row.get(1)?,
                active: row.get::<_, i64>(2)? != 0,
                system: row.get::<_, i64>(3)? != 0,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn ticket_department_is_active(&self, slug: &str) -> Result<bool, DbError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                "SELECT active FROM ticket_departments WHERE slug=?1",
                [slug],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .map(|active| active != 0)
            .unwrap_or(false))
    }

    pub fn ticket_department_exists(&self, slug: &str) -> Result<bool, DbError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                "SELECT 1 FROM ticket_departments WHERE slug=?1",
                [slug],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub fn create_ticket_department(
        &self,
        slug: &str,
        name: &str,
    ) -> Result<TicketDepartment, DbError> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute("INSERT INTO ticket_departments (slug,name,active,system,created_at,updated_at) VALUES (?1,?2,1,0,?3,?3)", params![slug, name, now])?;
        Ok(TicketDepartment {
            slug: slug.into(),
            name: name.into(),
            active: true,
            system: false,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn rename_ticket_department(
        &self,
        slug: &str,
        name: &str,
    ) -> Result<TicketDepartment, DbError> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        if conn.execute(
            "UPDATE ticket_departments SET name=?1, updated_at=?2 WHERE slug=?3",
            params![name, now, slug],
        )? == 0
        {
            return Err(DbError::NotFound);
        }
        conn.query_row("SELECT slug,name,active,system,created_at,updated_at FROM ticket_departments WHERE slug=?1", [slug], |row| Ok(TicketDepartment { slug:row.get(0)?, name:row.get(1)?, active:row.get::<_,i64>(2)? != 0, system:row.get::<_,i64>(3)? != 0, created_at:row.get(4)?, updated_at:row.get(5)? })) .map_err(DbError::from)
    }

    /// Arquivar bloqueia novos chamados e remove filas de recebimento, mas nao
    /// altera tickets anteriores nem seus vinculos historicos.
    pub fn archive_ticket_department(&self, slug: &str) -> Result<(), DbError> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        let system: Option<i64> = conn
            .query_row(
                "SELECT system FROM ticket_departments WHERE slug=?1",
                [slug],
                |r| r.get(0),
            )
            .optional()?;
        match system {
            None => return Err(DbError::NotFound),
            Some(value) if value != 0 => {
                return Err(DbError::InvalidInput(
                    "setor padrao nao pode ser arquivado".into(),
                ))
            }
            _ => {}
        }
        conn.execute(
            "UPDATE ticket_departments SET active=0, updated_at=?1 WHERE slug=?2",
            params![now, slug],
        )?;
        conn.execute(
            "DELETE FROM machine_ticket_departments WHERE department=?1",
            [slug],
        )?;
        conn.execute(
            "DELETE FROM portal_user_departments WHERE department=?1",
            [slug],
        )?;
        Ok(())
    }

    pub fn set_health_score(&self, machine_id: &str, score: u8) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE machines SET health_score=?1 WHERE id=?2",
            params![score, machine_id],
        )?;
        Ok(())
    }

    /// Substitui alertas ativos do PC pelo conjunto atual (sem duplicatas).
    pub fn sync_machine_alerts(
        &self,
        machine_id: &str,
        alerts: &[AlertRecord],
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM alerts WHERE machine_id = ?1 AND resolved = 0",
            [machine_id],
        )?;
        for alert in alerts {
            conn.execute(
                "INSERT INTO alerts (id, machine_id, severity, category, message, created_at, resolved)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)
                 ON CONFLICT(id) DO UPDATE SET
                 severity=excluded.severity, category=excluded.category, message=excluded.message,
                 created_at=excluded.created_at,
                 resolved=alerts.resolved",
                params![
                    alert.id,
                    alert.machine_id,
                    format!("{:?}", alert.severity).to_lowercase(),
                    alert.category,
                    alert.message,
                    alert.created_at.to_rfc3339(),
                ],
            )?;
        }
        Ok(())
    }

    pub fn get_alert_by_id(&self, id: &str) -> Result<AlertRecord, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT a.id, a.machine_id, m.hostname, a.severity, a.category, a.message, a.created_at, a.resolved
             FROM alerts a JOIN machines m ON m.id = a.machine_id WHERE a.id = ?1",
        )?;
        let row = stmt.query_row([id], |row| {
            let sev: String = row.get(3)?;
            let severity = match sev.as_str() {
                "critical" => AlertSeverity::Critical,
                "warning" => AlertSeverity::Warning,
                _ => AlertSeverity::Info,
            };
            Ok(AlertRecord {
                id: row.get(0)?,
                machine_id: row.get(1)?,
                hostname: row.get(2)?,
                severity,
                category: row.get(4)?,
                message: row.get(5)?,
                created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(6)?)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                resolved: row.get::<_, i32>(7)? != 0,
            })
        })?;
        Ok(row)
    }

    pub fn resolve_alert(&self, id: &str) -> Result<AlertRecord, DbError> {
        {
            let conn = self.conn.lock().unwrap();
            let n = conn.execute("UPDATE alerts SET resolved = 1 WHERE id = ?1", [id])?;
            if n == 0 {
                return Err(DbError::NotFound);
            }
        }
        self.get_alert_by_id(id)
    }

    /// Remove alertas duplicados legados (mesmo PC + categoria + mensagem).
    pub fn dedupe_alerts(&self) -> Result<u64, DbError> {
        let conn = self.conn.lock().unwrap();
        let deleted = conn.execute(
            "DELETE FROM alerts WHERE resolved = 0 AND rowid NOT IN (
                SELECT MAX(rowid) FROM alerts WHERE resolved = 0
                GROUP BY machine_id, category, message
            )",
            [],
        )?;
        Ok(deleted as u64)
    }

    pub fn list_alerts(&self, unresolved_only: bool) -> Result<Vec<AlertRecord>, DbError> {
        let _ = self.dedupe_alerts();
        let conn = self.conn.lock().unwrap();
        let sql = if unresolved_only {
            "SELECT a.id, a.machine_id, m.hostname, a.severity, a.category, a.message, a.created_at, a.resolved
             FROM alerts a JOIN machines m ON m.id = a.machine_id
             WHERE a.resolved = 0 ORDER BY a.created_at DESC"
        } else {
            "SELECT a.id, a.machine_id, m.hostname, a.severity, a.category, a.message, a.created_at, a.resolved
             FROM alerts a JOIN machines m ON m.id = a.machine_id
             ORDER BY a.created_at DESC LIMIT 200"
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([], |row| {
            let sev: String = row.get(3)?;
            let severity = match sev.as_str() {
                "critical" => AlertSeverity::Critical,
                "warning" => AlertSeverity::Warning,
                _ => AlertSeverity::Info,
            };
            Ok(AlertRecord {
                id: row.get(0)?,
                machine_id: row.get(1)?,
                hostname: row.get(2)?,
                severity,
                category: row.get(4)?,
                message: row.get(5)?,
                created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(6)?)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                resolved: row.get::<_, i32>(7)? != 0,
            })
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn create_agent_token(&self, hostname: Option<&str>) -> Result<String, DbError> {
        let token = uuid::Uuid::new_v4().to_string();
        let conn = self.conn.lock().unwrap();
        let now = Utc::now();
        let expires_at = (now + Duration::hours(24)).to_rfc3339();
        conn.execute(
            "INSERT INTO agent_tokens (token, hostname, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![token, hostname, now.to_rfc3339(), expires_at],
        )?;
        Ok(token)
    }

    pub fn get_server_config_json(&self) -> Result<serde_json::Value, DbError> {
        Ok(serde_json::to_value(belarc_shared::ServerConfig::default()).unwrap())
    }

    /// Persiste incidentes detectados — atualiza contagem se já existir (histórico).
    pub fn upsert_incidents(&self, incidents: &[IncidentRecord]) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        for inc in incidents {
            conn.execute(
                "INSERT INTO incident_events (id, machine_id, category, severity, title, message,
                 metric_value, threshold, recommendation, source_collector, observed_at, first_seen, last_seen, occurrence_count)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
                 ON CONFLICT(id) DO UPDATE SET
                 severity=excluded.severity,
                 message=excluded.message,
                 metric_value=excluded.metric_value,
                 threshold=excluded.threshold,
                 recommendation=excluded.recommendation,
                 last_seen=excluded.last_seen,
                 occurrence_count=incident_events.occurrence_count + 1",
                params![
                    inc.id,
                    inc.machine_id,
                    inc.category,
                    format!("{:?}", inc.severity).to_lowercase(),
                    inc.title,
                    inc.message,
                    inc.metric_value,
                    inc.threshold,
                    inc.recommendation,
                    inc.source_collector,
                    inc.observed_at.to_rfc3339(),
                    inc.first_seen.to_rfc3339(),
                    inc.last_seen.to_rfc3339(),
                    inc.occurrence_count,
                ],
            )?;
        }
        Ok(())
    }

    pub fn list_machine_incidents(
        &self,
        machine_id: &str,
        limit: u32,
    ) -> Result<Vec<IncidentRecord>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, machine_id, category, severity, title, message, metric_value, threshold,
             recommendation, source_collector, observed_at, first_seen, last_seen, occurrence_count
             FROM incident_events WHERE machine_id=?1 ORDER BY last_seen DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![machine_id, limit], |row| parse_incident_row(row))?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn list_all_incidents(&self, limit: u32) -> Result<Vec<IncidentRecord>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, machine_id, category, severity, title, message, metric_value, threshold,
             recommendation, source_collector, observed_at, first_seen, last_seen, occurrence_count
             FROM incident_events ORDER BY last_seen DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |row| parse_incident_row(row))?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn count_ti_users(&self) -> Result<usize, DbError> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM ti_users", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    pub fn create_ti_user(&self, username: &str, password_hash: &str) -> Result<String, DbError> {
        let conn = self.conn.lock().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO ti_users (id, username, password_hash, created_at) VALUES (?1,?2,?3,?4)",
            params![id, username, password_hash, now],
        )?;
        Ok(id)
    }

    pub fn get_ti_user_by_username(
        &self,
        username: &str,
    ) -> Result<Option<(String, String, String)>, DbError> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT id, username, password_hash FROM ti_users WHERE username=?1",
                [username],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        Ok(row)
    }

    pub fn create_ti_session(
        &self,
        user_id: &str,
        hours: i64,
    ) -> Result<(String, String), DbError> {
        let conn = self.conn.lock().unwrap();
        let token = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let expires = (now + Duration::hours(hours)).to_rfc3339();
        conn.execute(
            "INSERT INTO ti_sessions (token, user_id, expires_at, created_at) VALUES (?1,?2,?3,?4)",
            params![token, user_id, expires, now.to_rfc3339()],
        )?;
        // prune expired
        let _ = conn.execute(
            "DELETE FROM ti_sessions WHERE expires_at < ?1",
            [now.to_rfc3339()],
        );
        Ok((token, expires))
    }

    pub fn delete_ti_session(&self, token: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM ti_sessions WHERE token=?1", [token])?;
        Ok(())
    }

    /// Returns username if session is valid.
    pub fn validate_ti_session(&self, token: &str) -> Result<Option<String>, DbError> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let row = conn
            .query_row(
                "SELECT u.username FROM ti_sessions s
                 JOIN ti_users u ON u.id = s.user_id
                 WHERE s.token=?1 AND s.expires_at > ?2",
                params![token, now],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        Ok(row)
    }

    pub fn create_portal_user(
        &self,
        username: &str,
        display_name: &str,
        password_hash: &str,
        role: &str,
        department: Option<&str>,
    ) -> Result<PortalUser, DbError> {
        let username = username.trim();
        let display_name = display_name.trim();
        if username.is_empty() || display_name.is_empty() {
            return Err(DbError::InvalidInput(
                "usuario e nome sao obrigatorios".into(),
            ));
        }
        if !matches!(role, "requester" | "sector_agent") {
            return Err(DbError::InvalidInput("papel invalido".into()));
        }
        if role == "sector_agent" && department.is_none() {
            return Err(DbError::InvalidInput("atendente precisa de setor".into()));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute("INSERT INTO portal_users (id,username,display_name,password_hash,role,department,active,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,1,?7,?7)", params![id, username, display_name, password_hash, role, department, now])?;
        Ok(PortalUser {
            id,
            username: username.into(),
            display_name: display_name.into(),
            password_hash: password_hash.into(),
            role: role.into(),
            department: department.map(str::to_string),
        })
    }

    pub fn get_portal_user_by_username(
        &self,
        username: &str,
    ) -> Result<Option<PortalUser>, DbError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT id,username,display_name,password_hash,role,department FROM portal_users WHERE username=?1 AND active=1", [username.trim()], |r| Ok(PortalUser { id:r.get(0)?, username:r.get(1)?, display_name:r.get(2)?, password_hash:r.get(3)?, role:r.get(4)?, department:r.get(5)? })).optional().map_err(Into::into)
    }

    pub fn create_portal_session(
        &self,
        user_id: &str,
        hours: i64,
    ) -> Result<(String, String), DbError> {
        let token = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let expires = (now + Duration::hours(hours)).to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute("INSERT INTO portal_sessions (token,user_id,expires_at,created_at) VALUES (?1,?2,?3,?4)", params![token,user_id,expires,now.to_rfc3339()])?;
        let _ = conn.execute(
            "DELETE FROM portal_sessions WHERE expires_at < ?1",
            [now.to_rfc3339()],
        );
        Ok((token, expires))
    }

    pub fn delete_portal_session(&self, token: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM portal_sessions WHERE token=?1", [token])?;
        Ok(())
    }
    pub fn validate_portal_session(&self, token: &str) -> Result<Option<PortalUser>, DbError> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.query_row("SELECT u.id,u.username,u.display_name,u.password_hash,u.role,u.department FROM portal_sessions s JOIN portal_users u ON u.id=s.user_id WHERE s.token=?1 AND datetime(s.expires_at)>datetime(?2) AND u.active=1", params![token,now], |r| Ok(PortalUser { id:r.get(0)?,username:r.get(1)?,display_name:r.get(2)?,password_hash:r.get(3)?,role:r.get(4)?,department:r.get(5)? })).optional().map_err(Into::into)
    }
    pub fn link_portal_user_machine(&self, user_id: &str, machine_id: &str) -> Result<(), DbError> {
        let _ = self.get_machine_by_id(machine_id)?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO portal_user_machines (user_id,machine_id) VALUES (?1,?2)",
            params![user_id, machine_id],
        )?;
        Ok(())
    }
    pub fn portal_user_has_machine(
        &self,
        user_id: &str,
        machine_id: &str,
    ) -> Result<bool, DbError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                "SELECT 1 FROM portal_user_machines WHERE user_id=?1 AND machine_id=?2",
                params![user_id, machine_id],
                |_| Ok(true),
            )
            .optional()?
            .unwrap_or(false))
    }
    pub fn list_portal_user_machines(
        &self,
        user_id: &str,
    ) -> Result<Vec<(String, String)>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt=conn.prepare("SELECT m.id,m.hostname FROM portal_user_machines pm JOIN machines m ON m.id=pm.machine_id WHERE pm.user_id=?1 ORDER BY m.hostname")?;
        let rows = stmt.query_map([user_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.filter_map(Result::ok).collect())
    }
    pub fn set_portal_user_departments(
        &self,
        user_id: &str,
        departments: &[String],
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM portal_user_departments WHERE user_id=?1",
            [user_id],
        )?;
        for department in departments {
            conn.execute(
                "INSERT OR IGNORE INTO portal_user_departments (user_id,department) VALUES (?1,?2)",
                params![user_id, department],
            )?;
        }
        Ok(())
    }
    pub fn list_portal_user_departments(&self, user_id: &str) -> Result<Vec<String>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT department FROM portal_user_departments WHERE user_id=?1 ORDER BY department",
        )?;
        let rows = stmt.query_map([user_id], |r| r.get(0))?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    /// Conta técnica única por PC. Ela nunca recebe login por senha: somente
    /// o agente instalado pode solicitar uma sessão curta usando o token do PC.
    pub fn ensure_device_portal_user(
        &self,
        machine_id: &str,
        receive_departments: &[String],
        display_name: &str,
    ) -> Result<PortalUser, DbError> {
        let username = format!("device-{machine_id}-portal");
        let role = if receive_departments.is_empty() {
            "requester"
        } else {
            "sector_agent"
        };
        let department = receive_departments.first().map(String::as_str);
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        let existing = conn
            .query_row(
                "SELECT id,password_hash FROM portal_users WHERE username=?1",
                [&username],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?;
        let (id, password_hash) = match existing {
            Some(value) => value,
            None => (
                uuid::Uuid::new_v4().to_string(),
                "device-session-only".to_string(),
            ),
        };
        conn.execute("INSERT INTO portal_users (id,username,display_name,password_hash,role,department,active,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,1,?7,?7) ON CONFLICT(username) DO UPDATE SET display_name=excluded.display_name,role=excluded.role,department=excluded.department,active=1,updated_at=excluded.updated_at", params![id,username,display_name,password_hash,role,department,now])?;
        conn.execute(
            "INSERT OR IGNORE INTO portal_user_machines (user_id,machine_id) VALUES (?1,?2)",
            params![id, machine_id],
        )?;
        drop(conn);
        self.set_portal_user_departments(&id, receive_departments)?;
        Ok(PortalUser {
            id,
            username,
            display_name: display_name.to_string(),
            password_hash,
            role: role.to_string(),
            department: department.map(str::to_string),
        })
    }
}

fn parse_ticket_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<crate::tickets::Ticket> {
    let reopen_i: i64 = row.get(19).unwrap_or(0);
    Ok(crate::tickets::Ticket {
        id: row.get(0)?,
        code: row.get(1)?,
        machine_id: row.get(2)?,
        hostname_snapshot: row.get(3)?,
        owner_name_snapshot: row.get(4)?,
        title: row.get(5)?,
        description: row.get(6)?,
        status: row.get(7)?,
        priority: row.get(8)?,
        department: row.get(9)?,
        requester_user_id: row.get(10)?,
        created_by: row.get(11)?,
        assignee: row.get(12)?,
        nas_path: row.get(13)?,
        created_at: row.get(14)?,
        updated_at: row.get(15)?,
        closed_at: row.get(16)?,
        rating: row.get(17)?,
        rating_comment: row.get(18)?,
        reopen_pending: reopen_i != 0,
        reopen_reason: row.get(20)?,
        reopen_requested_at: row.get(21)?,
        last_reopen_reason: row.get(22)?,
        resolution: row.get(23)?,
        attachments: Vec::new(),
        comments: Vec::new(),
        checklist: Vec::new(),
    })
}

fn parse_incident_row(row: &rusqlite::Row<'_>) -> Result<IncidentRecord, rusqlite::Error> {
    let sev: String = row.get(3)?;
    let severity = match sev.as_str() {
        "critical" => AlertSeverity::Critical,
        "warning" => AlertSeverity::Warning,
        _ => AlertSeverity::Info,
    };
    Ok(IncidentRecord {
        id: row.get(0)?,
        machine_id: row.get(1)?,
        category: row.get(2)?,
        severity,
        title: row.get(4)?,
        message: row.get(5)?,
        metric_value: row.get(6)?,
        threshold: row.get(7)?,
        recommendation: row.get(8)?,
        source_collector: row.get(9)?,
        observed_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(10)?)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        first_seen: DateTime::parse_from_rfc3339(&row.get::<_, String>(11)?)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        last_seen: DateTime::parse_from_rfc3339(&row.get::<_, String>(12)?)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        occurrence_count: row.get::<_, i32>(13)? as u32,
    })
}

#[allow(dead_code)]
pub fn compute_collector_hash(data: &serde_json::Value) -> String {
    hash_json(data)
}
