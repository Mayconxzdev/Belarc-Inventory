#![cfg(windows)]

use belarc_setup::*;

fn main() {
    install_panic_hook();
    if ensure_ready_exe("BelarcServidorAtualizar.exe").is_some() {
        return;
    }

    let zip = include_bytes!(env!("BELARC_SERVIDOR_BUNDLE_ZIP"));
    let rt = tokio::runtime::Runtime::new().expect("tokio");
    run_server_repair(&rt, zip);
}
