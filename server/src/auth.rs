//! Auth TI — login/sessão para dados sensíveis (LGPD).

use std::path::Path;

use axum::http::HeaderMap;
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::db::Database;

pub const SESSION_HOURS: i64 = 12;
pub const BOOTSTRAP_USER: &str = "ti";

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub username: String,
    pub expires_at: String,
}

#[derive(Debug, Serialize)]
pub struct MeResponse {
    pub authenticated: bool,
    pub username: Option<String>,
}

pub fn hash_password(password: &str) -> Result<String, String> {
    bcrypt::hash(password, bcrypt::DEFAULT_COST).map_err(|e| e.to_string())
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    bcrypt::verify(password, hash).unwrap_or(false)
}

pub fn generate_password(len: usize) -> String {
    const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char)
        .collect()
}

/// Cria usuário TI inicial se a tabela estiver vazia.
pub fn bootstrap_ti_user(db: &Database, data_dir: &Path) -> Result<(), String> {
    let count = db.count_ti_users().map_err(|e| e.to_string())?;
    if count > 0 {
        return Ok(());
    }

    let username = std::env::var("BELARC_TI_USER").unwrap_or_else(|_| BOOTSTRAP_USER.into());
    let password = std::env::var("BELARC_TI_PASSWORD").unwrap_or_else(|_| generate_password(14));
    let hash = hash_password(&password)?;
    db.create_ti_user(&username, &hash)
        .map_err(|e| e.to_string())?;

    let note = format!(
        "Belarc TI — credencial inicial (gerada em {})\r\n\
         Usuario: {username}\r\n\
         Senha: {password}\r\n\
         \r\n\
         Altere apos o primeiro login. Apague este arquivo quando seguro.\r\n\
         Ou defina BELARC_TI_USER / BELARC_TI_PASSWORD antes de subir o servidor.\r\n",
        chrono::Utc::now().to_rfc3339()
    );
    let path = data_dir.join("ti-bootstrap.txt");
    std::fs::write(&path, note).map_err(|e| e.to_string())?;
    tracing::warn!(
        "TI user '{username}' criado. Credenciais em {}",
        path.display()
    );
    Ok(())
}

pub fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let auth = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let prefix = "Bearer ";
    if auth.len() > prefix.len() && auth[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(auth[prefix.len()..].trim().to_string())
    } else {
        None
    }
}

pub fn session_username(db: &Database, headers: &HeaderMap) -> Option<String> {
    let token = bearer_token(headers)?;
    db.validate_ti_session(&token).ok().flatten()
}

pub fn is_ti_authenticated(db: &Database, headers: &HeaderMap) -> bool {
    session_username(db, headers).is_some()
}
