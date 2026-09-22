#![cfg(windows)]

use belarc_setup::*;

fn main() {
    install_panic_hook();
    nas_pc_log(&format!(
        "main inicio argv={}",
        std::env::args().collect::<Vec<_>>().join(" ")
    ));

    if ensure_ready_exe("BelarcPC.exe").is_some() {
        nas_pc_log("main: reexecutado (UAC ou copia UNC) — encerrando processo pai");
        std::process::exit(0);
    }

    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("install");
    let zip = include_bytes!(env!("BELARC_PC_BUNDLE_ZIP"));
    let rt = tokio::runtime::Runtime::new().expect("tokio");

    match cmd {
        "install" | "update" => run_pc_install(zip),
        "repair" | "reparar" => run_pc_repair(zip),
        "status" | "verificar" => run_pc_status(),
        "install-http" | "http" => {
            let url = args.get(2).cloned().unwrap_or_else(default_server_url);
            run_pc_http_install(&rt, zip, &url);
        }
        "uninstall" => run_pc_uninstall(),
        "--help" | "-h" | "help" => print_help(),
        other => {
            eprintln!("Comando desconhecido: {other}");
            print_help();
            std::process::exit(1);
        }
    }
}

fn print_help() {
    println!(
        r#"BelarcPC — Inventario via NAS (PCs da rede)

Uso (duplo clique ou como Administrador):
  BelarcPC.exe                 Instala NAS (5 min + 6h) + primeira exportacao
  BelarcPC.exe repair          Repara tarefas e export NAS
  BelarcPC.exe status          Mostra tarefas e pasta NAS
  BelarcPC.exe install-http    Instala agente HTTP opcional (dashboard)
  BelarcPC.exe uninstall       Remove NAS + HTTP

NAS: \\FILE-SHARE\Portal\Belarc\PCs\<PC>\
Servidor: {}
"#,
        default_server_url()
    );
}
