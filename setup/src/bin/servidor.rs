#![cfg(windows)]

use belarc_setup::*;

fn main() {
    if ensure_ready_exe("BelarcServidor.exe").is_some() {
        std::process::exit(0);
    }

    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("install");
    let zip = include_bytes!(env!("BELARC_SERVIDOR_BUNDLE_ZIP"));
    let rt = tokio::runtime::Runtime::new().expect("tokio");

    match cmd {
        "install" | "update" => run_server_install(&rt, zip),
        "repair" | "reparar" => run_server_repair(&rt, zip),
        "status" | "verificar" => run_server_status(&rt),
        "uninstall" => run_server_uninstall(),
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
        r#"BelarcServidor — PC servidor TI

Uso (duplo clique ou como Administrador):
  BelarcServidor.exe           HTTP dashboard + agente local + NAS sync + portal
  BelarcServidor.exe repair    Reinicia servidor e re-sincroniza NAS
  BelarcServidor.exe status    Health HTTP e indice frota
  BelarcServidor.exe uninstall Remove tudo

Dashboard LAN: {}
Portal NAS: \\FILE-SHARE\Portal\Belarc\web\index.html
"#,
        default_server_url()
    );
}
