//! Belarc Inventory server.
#![allow(
    clippy::collapsible_if,
    clippy::manual_checked_ops,
    clippy::nonminimal_bool,
    clippy::ptr_arg,
    clippy::redundant_closure,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::unnecessary_sort_by
)]

mod admin;
mod auth;
mod compliance;
mod compliance_prompt;
mod db;
mod freeze_prone_apps;
mod incidents;
mod maintenance;
mod markdown;
mod routes;
mod standard_apps;
mod summary;
mod tickets;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::db::Database;

fn resolve_web_dir(data_dir: &PathBuf) -> PathBuf {
    // Preferir web em BELARC_DATA_DIR (atualizavel sem reinstalar o exe).
    let data_web = data_dir.join("web");
    if data_web.is_dir() && data_web.join("index.html").is_file() {
        return data_web;
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let bundled = dir.join("web");
            if bundled.is_dir() {
                return bundled;
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web")
}

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Database>,
    pub reports_dir: PathBuf,
    pub chamados_root: PathBuf,
    pub config: belarc_shared::ServerConfig,
    pub company_profile: belarc_shared::CompanyProfile,
    pub freeze_prone_apps: Vec<crate::freeze_prone_apps::FreezeProneApp>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("belarc_server=info".parse()?))
        .init();

    let data_dir = std::env::var("BELARC_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("./data"));

    std::fs::create_dir_all(&data_dir)?;
    let reports_dir = data_dir.join("reports");
    std::fs::create_dir_all(&reports_dir)?;

    let db_path = data_dir.join("belarc.db");
    let db = Arc::new(Database::open(&db_path)?);
    db.migrate()?;
    if let Err(e) = crate::auth::bootstrap_ti_user(&db, &data_dir) {
        tracing::error!("TI bootstrap failed: {e}");
    }
    if let Ok(n) = db.dedupe_alerts() {
        if n > 0 {
            tracing::info!("removed {n} duplicate alert(s) from database");
        }
    }

    let company_profile = belarc_shared::CompanyProfile::load();
    let freeze_prone_apps = crate::freeze_prone_apps::build_freeze_prone_apps(&company_profile);
    tracing::info!(
        "company profile: {} (ERP {} @ {})",
        company_profile.company_name,
        company_profile.erp_name,
        company_profile.erp_server_ip
    );

    let state = AppState {
        db: db.clone(),
        reports_dir,
        chamados_root: crate::tickets::chamados_root(),
        config: belarc_shared::ServerConfig::default(),
        company_profile,
        freeze_prone_apps,
    };
    tracing::info!("chamados root: {}", state.chamados_root.display());

    // Import inicial NAS → SQLite + gera _index.json / belarc-chamados.xlsx
    {
        let root = state.chamados_root.clone();
        let db_boot = state.db.clone();
        match crate::tickets::import_from_nas(&db_boot, &root) {
            Ok(s) => tracing::info!(
                "chamados NAS boot sync: scanned={} imported={} updated={} skipped={} errors={} export={}",
                s.scanned, s.imported, s.updated, s.skipped, s.errors, s.export_ok
            ),
            Err(e) => tracing::warn!("chamados NAS boot sync: {e}"),
        }
    }

    let web_dir = resolve_web_dir(&data_dir);
    tracing::info!("web dir: {}", web_dir.display());

    let app = Router::new()
        .merge(routes::api_router(state.clone()))
        .fallback_service(ServeDir::new(web_dir).append_index_html_on_directories(true))
        // Dashboard and API are same-origin by default. Deployments that need a
        // separate UI host must add a reviewed reverse-proxy policy instead of
        // inheriting permissive cross-origin access.
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = std::env::var("BELARC_LISTEN")
        .unwrap_or_else(|_| "0.0.0.0:80".into())
        .parse()?;

    tracing::info!("Belarc server listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
