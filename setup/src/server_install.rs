use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::nas::{
    deploy_server_nas_bundle, register_import_task, run_import_once, test_nas_share_access,
    uninstall_import_task,
};
use crate::{
    chrono_lite_now, create_token, default_server_url, extract_bundle, force_agent_collect,
    install_agent_service, install_repair_task, install_server_task, is_admin, listen_address,
    localhost_dashboard_url, localhost_server_url, open_firewall_port, pause, run_cmd,
    run_uninstall_core, server_task_exists, set_machine_env, start_existing_server_task,
    stop_running_installation, wait_server, AGENT_DATA, INSTALL_DIR, NAS_TASK_IMPORT, SERVER_DATA,
    SERVER_TASK,
};

pub fn run_server_install(rt: &tokio::runtime::Runtime, zip_bytes: &'static [u8]) {
    println!("========================================");
    println!("  Belarc Servidor TI");
    println!("  HTTP + NAS + Portal");
    println!("  Dashboard: {}", default_server_url());
    println!("========================================");
    println!();

    if !is_admin() {
        eprintln!("ERRO: Execute como Administrador.");
        pause("");
        std::process::exit(1);
    }

    let install_dir = PathBuf::from(INSTALL_DIR);
    let server_url = default_server_url();
    let listen = listen_address();

    println!("[1/8] Parando processos...");
    stop_running_installation();

    println!("[2/8] Extraindo arquivos...");
    if let Err(e) = extract_bundle(&install_dir, zip_bytes) {
        eprintln!("ERRO: {e}");
        pause("");
        std::process::exit(1);
    }

    println!("[3/8] Configurando firewall e dados...");
    fs::create_dir_all(SERVER_DATA).ok();
    fs::create_dir_all(format!("{SERVER_DATA}/reports")).ok();
    let _ = set_machine_env("BELARC_DATA_DIR", SERVER_DATA);
    let _ = set_machine_env("BELARC_LISTEN", &listen);
    let _ = open_firewall_port(crate::DEFAULT_PORT);

    println!("[4/8] Iniciando belarc-server...");
    if let Err(e) = install_server_task(&install_dir, &listen) {
        eprintln!("ERRO servidor: {e}");
        pause("");
        std::process::exit(1);
    }

    println!("[5/8] Aguardando HTTP...");
    let health_url = localhost_server_url();
    let ok = rt.block_on(wait_server(&health_url, 45));
    if !ok {
        eprintln!("AVISO: servidor demorou — aguarde 1 min");
    }

    println!("[6/8] Agente HTTP local...");
    let token = match rt.block_on(create_token(&health_url)) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("ERRO token: {e}");
            pause("");
            std::process::exit(1);
        }
    };
    let agent_exe = install_dir.join("belarc-agent.exe");
    if let Err(e) = install_agent_service(&agent_exe, &health_url, &token) {
        eprintln!("ERRO agente: {e}");
        pause("");
        std::process::exit(1);
    }
    let _ = install_repair_task(&install_dir);

    println!("[7/8] NAS: indexador + import + portal...");
    if test_nas_share_access() {
        println!("  NAS OK");
    } else {
        eprintln!("  AVISO: NAS inacessivel");
    }
    match deploy_server_nas_bundle(zip_bytes) {
        Ok(sync_script) => {
            if let Err(e) = register_import_task(&sync_script) {
                eprintln!("  AVISO tarefa NAS: {e}");
            } else {
                let _ = run_import_once(&sync_script);
            }
        }
        Err(e) => eprintln!("  AVISO deploy NAS: {e}"),
    }

    println!("[8/8] Coleta inicial servidor...");
    force_agent_collect(&install_dir);

    let info = serde_json::json!({
        "role": "server",
        "server_url": server_url,
        "dashboard_local": localhost_dashboard_url(),
        "dashboard_lan": server_url,
        "token": token,
        "hostname": std::env::var("COMPUTERNAME").unwrap_or_default(),
        "updated_at": chrono_lite_now(),
    });
    let _ = fs::write(
        std::path::Path::new(AGENT_DATA).join("install-info.json"),
        info.to_string(),
    );

    let _ = Command::new("cmd")
        .args(["/C", "start", "", &localhost_dashboard_url()])
        .spawn();

    println!();
    println!("=== SERVIDOR PRONTO ===");
    println!("  Dashboard LAN:  {server_url}");
    println!("  Dashboard local: {}", localhost_dashboard_url());
    println!("  Portal NAS: \\\\FILE-SHARE\\Portal\\Belarc\\web\\index.html");
    println!("  Tarefa NAS: {NAS_TASK_IMPORT}");
    pause("Concluido.");
}

pub fn run_server_repair(rt: &tokio::runtime::Runtime, zip_bytes: &'static [u8]) {
    println!("=== BelarcServidor — Reparo ===");
    println!();
    if !is_admin() {
        eprintln!("ERRO: Execute como Administrador.");
        pause("");
        std::process::exit(1);
    }

    println!("[1/3] Preparando atualização segura do belarc-server...");
    let install_dir = PathBuf::from(INSTALL_DIR);
    let preserve_server_task = server_task_exists();
    if preserve_server_task {
        println!(
            "  Tarefa existente {SERVER_TASK} será preservada, incluindo sua conta de execução."
        );
    }
    let data_dir = PathBuf::from(SERVER_DATA);
    let backup_dir = PathBuf::from(format!(
        "{SERVER_DATA}-backups\\pre-repair-{}",
        chrono_lite_now()
    ));
    if data_dir.is_dir() {
        println!(
            "[1/3] Criando backup do servidor em {}...",
            backup_dir.display()
        );
        if let Err(e) = crate::copy_dir_all(&data_dir, &backup_dir) {
            eprintln!("ERRO: backup não foi concluído: {e}");
            eprintln!("Nenhum arquivo do servidor foi substituído.");
            pause("");
            std::process::exit(1);
        }
    }
    stop_running_installation();
    if extract_bundle(&install_dir, zip_bytes).is_err() {
        eprintln!("AVISO: falha ao extrair bundle");
    }
    let listen = listen_address();
    let start_result = if preserve_server_task {
        start_existing_server_task()
    } else {
        println!("  Nenhuma tarefa anterior encontrada; criando inicialização padrão.");
        install_server_task(&install_dir, &listen)
    };
    if let Err(e) = start_result {
        eprintln!("ERRO: arquivos atualizados, mas o servidor não iniciou: {e}");
        eprintln!("O backup está em {}", backup_dir.display());
        pause("");
        std::process::exit(1);
    }
    if !rt.block_on(wait_server(&localhost_server_url(), 30)) {
        eprintln!("ERRO: o servidor não respondeu após a atualização.");
        eprintln!("O backup está em {}", backup_dir.display());
        pause("");
        std::process::exit(1);
    }

    println!("[2/3] NAS sync + portal...");
    if let Ok(sync) = deploy_server_nas_bundle(zip_bytes) {
        let _ = register_import_task(&sync);
        let _ = run_import_once(&sync);
    }

    println!("[3/3] OK");
    pause("");
}

pub fn run_server_status(rt: &tokio::runtime::Runtime) {
    println!("=== BelarcServidor — Status ===");
    println!();
    let health = localhost_server_url();
    let ok = rt.block_on(wait_server(&health, 5));
    println!("  HTTP: {}", if ok { "online" } else { "offline" });
    println!("  URL LAN: {}", default_server_url());

    if let Ok(o) = run_cmd("schtasks", &["/Query", "/TN", SERVER_TASK]) {
        if o.status.success() {
            println!("  Tarefa {SERVER_TASK}: OK");
        }
    }

    let ps = r#"
. 'C:\ProgramData\BelarcInventory\nas-import\config\belarc-deploy.ps1'
$idx = Get-BelarcNasFleetIndexPath
if ($idx -and (Test-Path $idx)) {
  $j = Get-Content $idx -Raw | ConvertFrom-Json
  Write-Host ('  Frota NAS: ' + $j.total + ' PCs, ' + $j.online_count + ' online')
} else { Write-Host '  Frota NAS: indice nao encontrado' -ForegroundColor Yellow }
"#;
    let _ = run_cmd("powershell.exe", &["-NoProfile", "-Command", ps]);
    println!();
    pause("");
}

pub fn run_server_uninstall() {
    if !is_admin() {
        eprintln!("Execute como Administrador.");
        std::process::exit(1);
    }
    run_uninstall_core(true);
    uninstall_import_task();
    println!("BelarcServidor removido.");
    pause("");
}
