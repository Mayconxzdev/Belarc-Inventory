mod cache;
mod client;
mod collector;
mod config;
mod scheduler;

use tracing_subscriber::EnvFilter;

fn init_logging(service_mode: bool) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let filter = EnvFilter::from_default_env().add_directive("belarc_agent=info".parse().unwrap());

    if service_mode {
        let file = tracing_appender::rolling::never(config::data_dir(), "agent.log");
        let (writer, guard) = tracing_appender::non_blocking(file);
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(writer)
            .with_ansi(false)
            .init();
        return Some(guard);
    }

    tracing_subscriber::fmt().with_env_filter(filter).init();
    None
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let service_mode = args.get(1).map(|s| s.as_str()) == Some("service")
        || std::env::var("BELARC_SERVICE").is_ok();
    let _log_guard = init_logging(service_mode);

    if args.len() > 1 {
        match args[1].as_str() {
            "run" => {
                scheduler::run_agent_loop().await?;
            }
            "collect" => {
                let tier = args.get(2).map(|s| s.as_str()).unwrap_or("t1");
                let force = args.iter().any(|a| a == "--force" || a == "-f");
                scheduler::run_collection_once(tier, force).await?;
            }
            "open-ticket" | "portal" | "attend-tickets" => open_portal().await?,
            "install" => {
                #[cfg(windows)]
                service::install_service()?;
                #[cfg(not(windows))]
                eprintln!("Service install only supported on Windows");
            }
            "uninstall" => {
                #[cfg(windows)]
                service::uninstall_service()?;
            }
            "service" => {
                #[cfg(windows)]
                service::run_service()?;
            }
            "--help" | "-h" => print_help(),
            other => {
                eprintln!("Unknown command: {other}");
                print_help();
            }
        }
    } else {
        #[cfg(windows)]
        {
            if std::env::var("BELARC_SERVICE").is_ok() {
                service::run_service()?;
            } else {
                scheduler::run_agent_loop().await?;
            }
        }
        #[cfg(not(windows))]
        scheduler::run_agent_loop().await?;
    }

    Ok(())
}

fn print_help() {
    println!(
        r#"Belarc Inventory Agent

Usage:
  belarc-agent              Run agent loop (foreground)
  belarc-agent run          Run agent loop
  belarc-agent collect [t1|t2|t3]  Run collection once
  belarc-agent open-ticket           Open ticket portal for this PC (no password)
  belarc-agent attend-tickets        Alias of open-ticket (compatibility)
  belarc-agent install      Install Windows Service
  belarc-agent uninstall    Remove Windows Service
  belarc-agent service      Run as Windows Service entrypoint

Environment:
  BELARC_SERVER_URL   Server base URL (default: http://127.0.0.1 — porta 80)
  BELARC_AGENT_TOKEN  Agent authentication token (required)
"#
    );
}

async fn open_portal() -> Result<(), Box<dyn std::error::Error>> {
    let config = config::AgentConfig::load();
    if config.agent_token.trim().is_empty() {
        return Err("agente sem token configurado".into());
    }
    let client = client::ServerClient::new(&config);
    let session = client.create_device_portal_session("portal").await?;
    let url = client.portal_url(&session);
    #[cfg(windows)]
    // `explorer.exe <URL>` pode abrir uma pasta do Explorer em alguns perfis
    // Windows. `start` delega a URL ao navegador padrão e preserva o fragmento
    // com a sessão limitada criada para este PC.
    {
        std::process::Command::new("cmd.exe")
            .args(["/C", "start", "", &url])
            .spawn()?;
    }
    #[cfg(not(windows))]
    {
        println!("Abra o portal no navegador configurado para este sistema.");
    }
    Ok(())
}

#[cfg(windows)]
mod service {
    use std::ffi::OsString;
    use std::time::Duration;

    use windows_service::define_windows_service;
    use windows_service::service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    use windows_service::service_dispatcher;

    define_windows_service!(ffi_service_main, service_main);

    const SERVICE_NAME: &str = "BelarcInventoryAgent";
    const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;

    pub fn install_service() -> Result<(), Box<dyn std::error::Error>> {
        use std::ffi::OsString;
        use std::process::Command;
        use std::thread;
        use std::time::Duration;
        use windows_service::service::{
            ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceType,
        };
        use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

        let exe = std::env::current_exe()?;

        let manager = ServiceManager::local_computer(
            None::<&str>,
            ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
        )?;

        if let Ok(existing) = manager.open_service(
            SERVICE_NAME,
            ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE,
        ) {
            let _ = existing.stop();
            thread::sleep(Duration::from_secs(2));
            let _ = existing.delete();
            thread::sleep(Duration::from_secs(1));
        }

        let service_info = ServiceInfo {
            name: OsString::from(SERVICE_NAME),
            display_name: OsString::from("Belarc Inventory Agent"),
            service_type: ServiceType::OWN_PROCESS,
            start_type: ServiceStartType::AutoStart,
            error_control: ServiceErrorControl::Normal,
            executable_path: exe,
            launch_arguments: vec![OsString::from("service")],
            dependencies: vec![],
            account_name: None,
            account_password: None,
        };

        let service = manager.create_service(
            &service_info,
            ServiceAccess::QUERY_STATUS | ServiceAccess::START | ServiceAccess::CHANGE_CONFIG,
        )?;

        let _ = Command::new("sc")
            .args([
                "description",
                SERVICE_NAME,
                "Inventario corporativo Belarc - coleta em segundo plano",
            ])
            .output();

        let _ = Command::new("sc")
            .args([
                "failure",
                SERVICE_NAME,
                "reset= 86400",
                "actions= restart/60000/restart/120000/restart/300000",
            ])
            .output();
        let _ = Command::new("sc")
            .args(["failureflag", SERVICE_NAME, "1"])
            .output();

        service.start(&[] as &[&OsString])?;

        println!("Service '{SERVICE_NAME}' installed and started.");
        println!("Log: {:?}", crate::config::data_dir().join("agent.log"));
        Ok(())
    }

    pub fn uninstall_service() -> Result<(), Box<dyn std::error::Error>> {
        use std::thread;
        use std::time::Duration;
        use windows_service::service::ServiceAccess;
        use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;

        match manager.open_service(
            SERVICE_NAME,
            ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE,
        ) {
            Ok(service) => {
                let _ = service.stop();
                thread::sleep(Duration::from_secs(2));
                service.delete()?;
            }
            Err(e) => {
                return Err(format!("service not found or access denied: {e}").into());
            }
        }

        println!("Service '{SERVICE_NAME}' removed.");
        Ok(())
    }

    pub fn run_service() -> Result<(), Box<dyn std::error::Error>> {
        service_dispatcher::start(SERVICE_NAME, ffi_service_main)?;
        Ok(())
    }

    fn service_main(_arguments: Vec<OsString>) {
        if let Err(e) = run_service_impl() {
            tracing::error!("service error: {e}");
        }
    }

    fn run_service_impl() -> Result<(), Box<dyn std::error::Error>> {
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::channel::<()>(1);

        let event_handler = move |control_event| -> ServiceControlHandlerResult {
            match control_event {
                ServiceControl::Stop | ServiceControl::Shutdown => {
                    let _ = shutdown_tx.blocking_send(());
                    ServiceControlHandlerResult::NoError
                }
                ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
                _ => ServiceControlHandlerResult::NotImplemented,
            }
        };

        let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)?;

        status_handle.set_service_status(ServiceStatus {
            service_type: SERVICE_TYPE,
            current_state: ServiceState::Running,
            controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        })?;

        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async {
            tokio::select! {
                result = crate::scheduler::run_agent_loop() => {
                    if let Err(e) = result {
                        tracing::error!("agent loop failed: {e}");
                    }
                }
                _ = shutdown_rx.recv() => {
                    tracing::info!("service shutdown requested");
                }
            }
        });

        status_handle.set_service_status(ServiceStatus {
            service_type: SERVICE_TYPE,
            current_state: ServiceState::Stopped,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        })?;

        Ok(())
    }
}
