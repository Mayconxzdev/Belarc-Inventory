use std::fs;
use std::path::Path;

use crate::nas::{
    deploy_http_from_pc_bundle, deploy_pc_bundle, nas_task_principal_user, register_pc_tasks,
    run_script_background, run_script_wait, test_nas_share_access, uninstall_pc_tasks,
    verify_presence_on_share,
};
use crate::{
    chrono_lite_now, create_token, force_agent_collect_wait, install_agent_service,
    install_ticket_portal_shortcut, is_admin, nas_pc_log, pause, remove_legacy_http_agent,
    verify_machine_on_server, AGENT_DATA, NAS_TASK_FULL, NAS_TASK_FULL_BOOT, NAS_TASK_PRESENCE,
};

pub fn run_pc_install(zip_bytes: &'static [u8]) {
    nas_pc_log("=== Inicio BelarcPC install ===");
    println!("========================================");
    println!("  Belarc PC - Inventario via NAS");
    println!("========================================");
    println!();
    println!("Log: C:\\ProgramData\\BelarcInventory\\nas-pc-install.log");
    println!();

    if !is_admin() {
        nas_pc_log("ERRO: sem admin");
        eprintln!("ERRO: Execute como Administrador.");
        pause("");
        std::process::exit(1);
    }

    let hostname = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into());
    nas_pc_log(&format!("PC: {hostname}"));

    println!("[1/6] Removendo agente HTTP antigo...");
    nas_pc_log("passo 1: remove agente HTTP");
    remove_legacy_http_agent();
    println!("  OK");
    nas_pc_log("passo 1: ok");

    println!("[2/6] Testando pasta NAS...");
    nas_pc_log("passo 2: teste NAS");
    if test_nas_share_access() {
        println!("  OK: NAS acessivel");
    } else {
        eprintln!("  AVISO: NAS inacessivel agora");
    }

    println!("[3/6] Instalando coletores e scripts...");
    let export_dir = match deploy_pc_bundle(zip_bytes) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("ERRO: {e}");
            pause("");
            std::process::exit(1);
        }
    };

    println!("[4/6] Agendando tarefas (5 min + 6h)...");
    if let Err(e) = register_pc_tasks(&export_dir) {
        eprintln!("ERRO tarefa: {e}");
        pause("");
        std::process::exit(1);
    }

    println!("[5/6] Primeira presenca no NAS...");
    let presence_script = export_dir.join("exportar-presence-nas.ps1");
    match run_script_wait(&presence_script) {
        Ok(_) if verify_presence_on_share(&hostname) => {
            println!("  OK: presence.json no NAS");
        }
        Ok(_) => {
            eprintln!("  AVISO: presence nao encontrado no NAS");
            eprintln!("  Veja: C:\\ProgramData\\BelarcInventory\\nas-export.log");
        }
        Err(e) => {
            eprintln!("  ERRO: {e}");
            eprintln!("  Veja: C:\\ProgramData\\BelarcInventory\\nas-export.log");
        }
    }

    println!("[6/6] Coleta completa em segundo plano...");
    run_script_background(&export_dir.join("exportar-para-nas.ps1"));

    write_pc_info(&hostname, &export_dir);
    print_pc_done(&hostname);
    nas_pc_log("=== Instalacao concluida ===");
    pause("Concluido.");
}

pub fn run_pc_repair(zip_bytes: &'static [u8]) {
    println!("=== BelarcPC — Reparo ===");
    println!();

    if !is_admin() {
        eprintln!("ERRO: Execute como Administrador.");
        pause("");
        std::process::exit(1);
    }

    let hostname = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into());

    println!("[1/4] Reinstalando scripts...");
    let export_dir = match deploy_pc_bundle(zip_bytes) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("ERRO: {e}");
            pause("");
            std::process::exit(1);
        }
    };

    println!(
        "[2/4] Recriando tarefas (usuario {})...",
        nas_task_principal_user()
    );
    if let Err(e) = register_pc_tasks(&export_dir) {
        eprintln!("ERRO: {e}");
        pause("");
        std::process::exit(1);
    }

    println!("[3/4] Export presence...");
    let presence = export_dir.join("exportar-presence-nas.ps1");
    match run_script_wait(&presence) {
        Ok(_) => println!("  OK"),
        Err(e) => eprintln!("  ERRO: {e}"),
    }

    println!("[4/4] Verificando NAS...");
    if verify_presence_on_share(&hostname) {
        println!("  OK: \\\\FILE-SHARE\\Portal\\Belarc\\PCs\\{hostname}");
    } else {
        eprintln!("  FALHOU — veja nas-export.log");
        show_export_log_tail();
    }
    pause("");
}

pub fn run_pc_status() {
    println!("=== BelarcPC — Status ===");
    println!();
    let ps = r#"
. 'C:\ProgramData\BelarcInventory\nas-export\config\belarc-deploy.ps1'
Write-Host '[Tarefas]' -ForegroundColor Yellow
foreach ($t in @('BelarcInventoryNasPresence','BelarcInventoryNasFull','BelarcInventoryNasFullBoot')) {
  $task = Get-ScheduledTask -TaskName $t -ErrorAction SilentlyContinue
  if ($task) {
    $info = Get-ScheduledTaskInfo -TaskName $t -ErrorAction SilentlyContinue
    $last = if ($info.LastRunTime) { $info.LastRunTime.ToString('yyyy-MM-dd HH:mm') } else { 'nunca' }
    Write-Host ('  OK: ' + $t + ' | ' + $task.State + ' | Ultima=' + $last) -ForegroundColor Green
  } else { Write-Host ('  FALTA: ' + $t) -ForegroundColor Red }
}
$folder = Get-BelarcNasPcFolder
Write-Host ''
Write-Host '[NAS]' -ForegroundColor Yellow
if ($folder -and (Test-Path -LiteralPath $folder)) {
  Write-Host ('  ' + $folder) -ForegroundColor White
  if (Test-Path (Join-Path $folder 'presence.json')) { Write-Host '  presence.json: OK' -ForegroundColor Green }
  if (Test-Path (Join-Path $folder 'inventory_t1.json')) { Write-Host '  inventory_t1.json: OK' -ForegroundColor Green }
} else { Write-Host '  pasta ainda nao criada' -ForegroundColor Yellow }
$log = 'C:\ProgramData\BelarcInventory\nas-export.log'
if (Test-Path $log) {
  Write-Host ''
  Write-Host '[Log export]' -ForegroundColor Yellow
  Get-Content $log -Tail 6 | ForEach-Object { Write-Host ('  ' + $_) }
}
"#;
    let _ = crate::run_cmd(
        "powershell.exe",
        &["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", ps],
    );
    println!();
    pause("");
}

pub fn run_pc_http_install(
    rt: &tokio::runtime::Runtime,
    zip_bytes: &'static [u8],
    server_url: &str,
) {
    println!("========================================");
    println!("  Belarc PC — Agente HTTP (opcional)");
    println!("  Servidor: {server_url}");
    println!("  (NAO remove modo NAS se ja instalado)");
    println!("========================================");
    println!();

    if !is_admin() {
        eprintln!("ERRO: Execute como Administrador.");
        pause("");
        std::process::exit(1);
    }

    let install_dir = match deploy_http_from_pc_bundle(zip_bytes) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("ERRO: {e}");
            pause("");
            std::process::exit(1);
        }
    };

    println!("[1/4] Verificando servidor...");
    if !rt.block_on(crate::wait_server(server_url, 25)) {
        eprintln!("AVISO: servidor nao responde ainda");
    }

    println!("[2/4] Obtendo token...");
    let token = match rt.block_on(create_token(server_url)) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("ERRO: {e}");
            pause("");
            std::process::exit(1);
        }
    };

    println!("[3/4] Instalando servico HTTP...");
    let agent_exe = install_dir.join("belarc-agent.exe");
    if let Err(e) = install_agent_service(&agent_exe, server_url, &token) {
        eprintln!("ERRO: {e}");
        pause("");
        std::process::exit(1);
    }
    if let Err(e) = install_ticket_portal_shortcut(&agent_exe) {
        eprintln!("AVISO: atalho Chamados Serviços não criado: {e}");
    } else {
        println!("  Atalho criado: Área de Trabalho Pública\\Chamados Serviços");
    }

    println!("[4/4] Coleta inicial...");
    let _ = force_agent_collect_wait(&install_dir, 360);

    let hostname = std::env::var("COMPUTERNAME").unwrap_or_default();
    if rt.block_on(verify_machine_on_server(server_url, &hostname, 30)) {
        println!("OK: {hostname} no dashboard {server_url}");
    } else {
        eprintln!("AVISO: verifique o dashboard");
    }
    pause("Concluido.");
}

pub fn run_pc_uninstall() {
    if !is_admin() {
        eprintln!("Execute como Administrador.");
        std::process::exit(1);
    }
    remove_legacy_http_agent();
    uninstall_pc_tasks();
    println!("BelarcPC removido (NAS + HTTP).");
    pause("");
}

fn write_pc_info(hostname: &str, export_dir: &Path) {
    let info = serde_json::json!({
        "role": "nas_pc",
        "hostname": hostname,
        "export_dir": export_dir.to_string_lossy(),
        "nas_folder": format!(r"\\FILE-SHARE\Portal\Belarc\PCs\{hostname}"),
        "updated_at": chrono_lite_now(),
    });
    let _ = fs::write(
        Path::new(AGENT_DATA).join("nas-pc-install.json"),
        info.to_string(),
    );
}

fn print_pc_done(hostname: &str) {
    println!();
    println!("=== INSTALADO ===");
    println!("  PC: {hostname}");
    println!("  Presenca: {NAS_TASK_PRESENCE} (5 min)");
    println!("  Completa: {NAS_TASK_FULL} + {NAS_TASK_FULL_BOOT}");
    println!("  NAS: \\\\FILE-SHARE\\Portal\\Belarc\\PCs\\{hostname}");
    println!("  Distrib: \\\\FILE-SHARE\\Portal\\Belarc\\distrib\\");
    println!("  HTTP opcional: BelarcPC.exe install-http");
}

fn show_export_log_tail() {
    let log = Path::new(AGENT_DATA).join("nas-export.log");
    if log.is_file() {
        if let Ok(s) = fs::read_to_string(&log) {
            for line in s
                .lines()
                .rev()
                .take(10)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
            {
                println!("  {line}");
            }
        }
    }
}
