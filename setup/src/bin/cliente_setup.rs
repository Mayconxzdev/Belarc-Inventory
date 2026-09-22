#![cfg(windows)]

use belarc_setup::*;

fn main() {
    install_panic_hook();
    if ensure_ready_exe("BelarcClienteSetup.exe").is_some() {
        return;
    }
    let args: Vec<String> = std::env::args().collect();
    let default_url = default_server_url();
    let server_url = args
        .windows(2)
        .find(|w| w[0] == "--server-url")
        .map(|w| w[1].as_str())
        .unwrap_or(&default_url);
    let pc_bundle = include_bytes!(env!("BELARC_PC_BUNDLE_ZIP"));
    let ticket_v2 = include_bytes!(env!("BELARC_TICKET_V2_BUNDLE"));
    let ticket_icon = include_bytes!(env!("BELARC_TICKET_ICON_BUNDLE"));
    let rt = tokio::runtime::Runtime::new().expect("tokio");
    run_client_setup(&rt, pc_bundle, ticket_v2, ticket_icon, server_url);
}
