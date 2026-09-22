#![cfg(windows)]

mod elevate;
mod nas;
mod pc_install;
mod role_setup;
mod server_install;

pub use elevate::ensure_ready_exe;
pub use nas::test_nas_share_access;
pub use pc_install::{
    run_pc_http_install, run_pc_install, run_pc_repair, run_pc_status, run_pc_uninstall,
};
pub use role_setup::{run_client_setup, run_ti_setup};
pub use server_install::{
    run_server_install, run_server_repair, run_server_status, run_server_uninstall,
};

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

pub const DEFAULT_SERVER_IP: &str = "192.0.2.10";
pub const DEFAULT_PORT: u16 = 80;
pub const INSTALL_DIR: &str = r"C:\Program Files\BelarcInventory";
pub const COMPANY_PROFILE: &str = r"C:\Program Files\BelarcInventory\config\company-profile.json";
pub const SERVER_DATA: &str = r"C:\ProgramData\BelarcInventoryServer";
pub const AGENT_DATA: &str = r"C:\ProgramData\BelarcInventory";
pub const SERVER_TASK: &str = "BelarcInventoryServer";
pub const AGENT_SERVICE: &str = "BelarcInventoryAgent";
pub const REPAIR_TASK: &str = "BelarcInventoryRepair";
pub const NAS_EXPORT_DIR: &str = r"C:\ProgramData\BelarcInventory\nas-export";
pub const NAS_IMPORT_DIR: &str = r"C:\ProgramData\BelarcInventory\nas-import";
pub const NAS_TASK_PRESENCE: &str = "BelarcInventoryNasPresence";
pub const NAS_TASK_FULL: &str = "BelarcInventoryNasFull";
pub const NAS_TASK_FULL_BOOT: &str = "BelarcInventoryNasFullBoot";
pub const NAS_TASK_IMPORT: &str = "BelarcInventoryNasImport";

pub fn default_server_url() -> String {
    if DEFAULT_PORT == 80 {
        format!("http://{DEFAULT_SERVER_IP}")
    } else {
        format!("http://{DEFAULT_SERVER_IP}:{DEFAULT_PORT}")
    }
}

pub fn localhost_server_url() -> String {
    if DEFAULT_PORT == 80 {
        "http://127.0.0.1".into()
    } else {
        format!("http://127.0.0.1:{DEFAULT_PORT}")
    }
}

pub fn localhost_dashboard_url() -> String {
    if DEFAULT_PORT == 80 {
        "http://localhost".into()
    } else {
        format!("http://localhost:{DEFAULT_PORT}")
    }
}

pub fn listen_address() -> String {
    format!("0.0.0.0:{DEFAULT_PORT}")
}

pub fn is_admin() -> bool {
    Command::new("net")
        .args(["session"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn pause(msg: &str) {
    println!();
    if !msg.is_empty() {
        println!("{msg}");
    }
    println!("Pressione qualquer tecla para fechar...");
    let _ = std::io::stdout().flush();
    let _ = Command::new("cmd").args(["/C", "pause"]).status();
}

pub fn nas_pc_log(msg: &str) {
    let log_path = Path::new(AGENT_DATA).join("nas-pc-install.log");
    let _ = fs::create_dir_all(AGENT_DATA);
    let line = format!("[{}] {}\n", log_timestamp(), msg);
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        let _ = f.write_all(line.as_bytes());
        let _ = f.flush();
    }
}

pub fn log_timestamp() -> String {
    let ps = r#"
try {
  (Get-Date).ToString('yyyy-MM-dd HH:mm:ss')
} catch {
  [DateTime]::UtcNow.ToString('yyyy-MM-dd HH:mm:ss')
}
"#;
    if let Ok(o) = Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", ps])
        .output()
    {
        let t = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if !t.is_empty() {
            return t;
        }
    }
    chrono_lite_now()
}

pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("PANIC: {info}");
        nas_pc_log(&msg);
        eprintln!("{msg}");
        pause("Erro fatal.");
    }));
}

pub fn run_cmd(program: &str, args: &[&str]) -> Result<std::process::Output, String> {
    Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("{program} falhou: {e}"))
}

pub fn cmd_output_text(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("codigo {}", output.status)
    }
}

pub fn run_powershell_hidden(script: &str) -> Result<(), String> {
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-Command",
            script,
        ])
        .output()
        .map_err(|e| format!("powershell falhou: {e}"))?;
    if !output.status.success() {
        return Err(cmd_output_text(&output));
    }
    Ok(())
}

pub fn stop_running_installation() {
    let _ = run_cmd("schtasks", &["/End", "/TN", SERVER_TASK]);
    let _ = run_cmd("sc", &["stop", AGENT_SERVICE]);
    for _ in 0..3 {
        let _ = run_cmd("taskkill", &["/F", "/IM", "belarc-server.exe"]);
        let _ = run_cmd("taskkill", &["/F", "/IM", "belarc-agent.exe"]);
        thread::sleep(Duration::from_secs(2));
    }
}

pub fn extract_zip_to_dir(dest: &Path, zip_bytes: &[u8]) -> Result<(), String> {
    if zip_bytes.len() < 100 {
        return Err("Pacote interno vazio. Recompile com: .\\build-publish.ps1".into());
    }
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let cursor = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| format!("ZIP invalido: {e}"))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let outpath = dest.join(file.name());
        if file.name().ends_with('/') {
            fs::create_dir_all(&outpath).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = outpath.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        if outpath.is_file() {
            let _ = fs::remove_file(&outpath);
        }
        let mut outfile = File::create(&outpath).map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        outfile.write_all(&buf).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(crate) fn copy_dir_all(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let dest = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir_all(&path, &dest)?;
        } else {
            fs::copy(&path, &dest).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub(crate) fn copy_file_if_exists(src: &Path, dst: &Path) -> Result<(), String> {
    if src.is_file() {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::copy(src, dst).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn extract_bundle(dest: &Path, zip_bytes: &[u8]) -> Result<(), String> {
    stop_running_installation();
    if zip_bytes.len() < 100 {
        return Err("Pacote interno vazio. Recompile com: .\\build-publish.ps1".into());
    }
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let cursor = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| format!("ZIP invalido: {e}"))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let outpath = dest.join(file.name());
        if file.name().ends_with('/') {
            fs::create_dir_all(&outpath).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = outpath.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        if outpath.is_file() {
            let remove = fs::remove_file(&outpath);
            if remove.is_err() {
                stop_running_installation();
                fs::remove_file(&outpath).map_err(|e| {
                    format!("nao foi possivel substituir {}: {e}", outpath.display())
                })?;
            }
        }
        let mut outfile = File::create(&outpath).map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        outfile.write_all(&buf).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn set_machine_env(key: &str, value: &str) -> Result<(), String> {
    run_cmd("setx", &[key, value, "/M"])?;
    Ok(())
}

pub fn open_firewall_port(port: u16) -> Result<(), String> {
    let rule = format!("Belarc Inventory TCP {port}");
    let _ = run_cmd(
        "netsh",
        &[
            "advfirewall",
            "firewall",
            "delete",
            "rule",
            "name=Belarc Inventory TCP 8080",
        ],
    );
    let _ = run_cmd(
        "netsh",
        &[
            "advfirewall",
            "firewall",
            "delete",
            "rule",
            &format!("name={rule}"),
        ],
    );
    run_cmd(
        "netsh",
        &[
            "advfirewall",
            "firewall",
            "add",
            "rule",
            &format!("name={rule}"),
            "dir=in",
            "action=allow",
            "protocol=TCP",
            &format!("localport={port}"),
            "profile=any",
        ],
    )?;
    let ps = format!(
        "New-NetFirewallRule -DisplayName '{rule}' -Direction Inbound -Action Allow -Protocol TCP -LocalPort {port} -Profile Domain,Private,Public -Enabled True -ErrorAction SilentlyContinue"
    );
    let _ = run_cmd("powershell", &["-NoProfile", "-Command", &ps]);
    Ok(())
}

pub fn install_server_task(install_dir: &Path, listen: &str) -> Result<(), String> {
    let run_as = std::env::var("BELARC_SERVER_RUN_AS").ok();
    let password = std::env::var("BELARC_SERVER_RUN_AS_PASSWORD").ok();
    install_server_task_as(install_dir, listen, run_as.as_deref(), password.as_deref())
}

pub fn server_task_exists() -> bool {
    run_cmd("schtasks", &["/Query", "/TN", SERVER_TASK])
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub fn start_existing_server_task() -> Result<(), String> {
    let output = run_cmd("schtasks", &["/Run", "/TN", SERVER_TASK])?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "não foi possível iniciar a tarefa existente {SERVER_TASK}: {}",
            cmd_output_text(&output)
        ))
    }
}

/// Cria tarefa ONSTART do belarc-server. Com `run_as` usa conta com acesso ao NAS (nao SYSTEM).
pub fn install_server_task_as(
    install_dir: &Path,
    listen: &str,
    run_as: Option<&str>,
    password: Option<&str>,
) -> Result<(), String> {
    let exe = install_dir.join("belarc-server.exe");
    let exe_str = exe.to_string_lossy();
    let chamados_root = std::env::var("BELARC_CHAMADOS_ROOT")
        .unwrap_or_else(|_| r"\\FILE-SHARE\Portal\Chamados".into());
    let tr = format!(
        "cmd /c \"set BELARC_DATA_DIR={SERVER_DATA}&& set BELARC_LISTEN={listen}&& set BELARC_COMPANY_PROFILE={COMPANY_PROFILE}&& set BELARC_CHAMADOS_ROOT={chamados_root}&& \"{exe_str}\"\""
    );
    let _ = run_cmd("schtasks", &["/End", "/TN", SERVER_TASK]);
    let _ = run_cmd("schtasks", &["/Delete", "/F", "/TN", SERVER_TASK]);

    if let Some(user) = run_as.filter(|u| !u.trim().is_empty()) {
        let u = user.trim();
        if let Some(pw) = password.filter(|p| !p.is_empty()) {
            run_cmd(
                "schtasks",
                &[
                    "/Create",
                    "/F",
                    "/TN",
                    SERVER_TASK,
                    "/TR",
                    &tr,
                    "/SC",
                    "ONSTART",
                    "/RU",
                    u,
                    "/RP",
                    pw,
                    "/RL",
                    "HIGHEST",
                ],
            )?;
        } else {
            run_cmd(
                "schtasks",
                &[
                    "/Create",
                    "/F",
                    "/TN",
                    SERVER_TASK,
                    "/TR",
                    &tr,
                    "/SC",
                    "ONSTART",
                    "/RU",
                    u,
                    "/RL",
                    "HIGHEST",
                ],
            )?;
        }
    } else {
        run_cmd(
            "schtasks",
            &[
                "/Create",
                "/F",
                "/TN",
                SERVER_TASK,
                "/TR",
                &tr,
                "/SC",
                "ONSTART",
                "/RU",
                "SYSTEM",
                "/RL",
                "HIGHEST",
            ],
        )?;
    }
    run_cmd("schtasks", &["/Run", "/TN", SERVER_TASK])?;
    Ok(())
}

pub fn install_repair_task(install_dir: &Path) -> Result<(), String> {
    let repair_script = install_dir.join("reparo-automatico.ps1");
    if !repair_script.is_file() {
        return Ok(());
    }
    let script_str = repair_script.to_string_lossy();
    let tr = format!(
        "powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File \"{script_str}\""
    );
    let _ = run_cmd("schtasks", &["/End", "/TN", REPAIR_TASK]);
    let _ = run_cmd("schtasks", &["/Delete", "/F", "/TN", REPAIR_TASK]);
    run_cmd(
        "schtasks",
        &[
            "/Create",
            "/F",
            "/TN",
            REPAIR_TASK,
            "/TR",
            &tr,
            "/SC",
            "HOURLY",
            "/MO",
            "4",
            "/RU",
            "SYSTEM",
            "/RL",
            "HIGHEST",
        ],
    )?;
    Ok(())
}

pub fn install_agent_service(
    agent_exe: &Path,
    server_url: &str,
    token: &str,
) -> Result<(), String> {
    fs::create_dir_all(AGENT_DATA).map_err(|e| e.to_string())?;
    let config = format!(
        r#"server_url = "{server_url}"
agent_token = "{token}"
heartbeat_interval_seconds = 180
t1_interval_seconds = 21600
t2_interval_seconds = 86400
"#
    );
    fs::write(Path::new(AGENT_DATA).join("config.toml"), config).map_err(|e| e.to_string())?;
    let _ = run_cmd("sc", &["stop", AGENT_SERVICE]);
    let _ = run_cmd("sc", &["delete", AGENT_SERVICE]);
    thread::sleep(Duration::from_secs(3));
    let output = Command::new(agent_exe)
        .arg("install")
        .output()
        .map_err(|e| format!("belarc-agent install falhou: {e}"))?;
    if !output.status.success() {
        return install_agent_service_powershell(agent_exe);
    }
    Ok(())
}

/// Atalho simples para o usuário. O token nunca entra no atalho: o agente lê
/// a configuração protegida em ProgramData e cria uma sessão temporária para
/// o PC que está abrindo o chamado.
pub fn install_ticket_portal_shortcut(agent_exe: &Path) -> Result<(), String> {
    if !agent_exe.is_file() {
        return Err("belarc-agent.exe nao encontrado para criar atalho de chamados".into());
    }
    let exe = agent_exe.to_string_lossy().replace('\'', "''");
    let workdir = agent_exe
        .parent()
        .unwrap_or_else(|| Path::new(INSTALL_DIR))
        .to_string_lossy()
        .replace('\'', "''");
    let ps = format!(
        r#"
$desktop = [Environment]::GetFolderPath('CommonDesktopDirectory')
$path = Join-Path $desktop 'Chamados Serviços.lnk'
$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut($path)
$shortcut.TargetPath = '{exe}'
$shortcut.Arguments = 'open-ticket'
$shortcut.WorkingDirectory = '{workdir}'
$shortcut.IconLocation = '{exe},0'
$shortcut.Description = 'Abrir chamados deste computador'
$shortcut.Save()
"#
    );
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps])
        .output()
        .map_err(|e| format!("atalho Chamados Serviços: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "atalho Chamados Serviços: {}",
            cmd_output_text(&output)
        ));
    }
    Ok(())
}

pub fn remove_ticket_portal_shortcut() {
    let ps = r#"
$path = Join-Path ([Environment]::GetFolderPath('CommonDesktopDirectory')) 'Chamados Serviços.lnk'
Remove-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
"#;
    let _ = run_powershell_hidden(ps);
}

fn install_agent_service_powershell(agent_exe: &Path) -> Result<(), String> {
    let exe_str = agent_exe.to_string_lossy().replace('\'', "''");
    let ps = format!(
        r#"
$svc = '{AGENT_SERVICE}'
$existing = Get-Service -Name $svc -ErrorAction SilentlyContinue
if ($existing) {{
    Stop-Service -Name $svc -Force -ErrorAction SilentlyContinue
    sc.exe delete $svc | Out-Null
    Start-Sleep -Seconds 3
}}
New-Service -Name $svc -BinaryPathName '"{exe_str}" service' -DisplayName 'Belarc Inventory Agent' -StartupType Automatic -Description 'Belarc Inventory Agent' | Out-Null
Start-Service -Name $svc
"#
    );
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps])
        .output()
        .map_err(|e| format!("New-Service falhou: {e}"))?;
    if !output.status.success() {
        return Err(format!("New-Service: {}", cmd_output_text(&output)));
    }
    Ok(())
}

pub async fn wait_server(url: &str, secs: u64) -> bool {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    for _ in 0..secs {
        if client
            .get(format!("{url}/api/health"))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
        {
            return true;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    false
}

pub async fn create_token(url: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let res = client
        .post(format!("{url}/api/tokens"))
        .header("Content-Type", "application/json")
        .body("{}")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("HTTP {}", res.status()));
    }
    let json: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
    json.get("token")
        .and_then(|t| t.as_str())
        .map(String::from)
        .ok_or_else(|| "resposta sem token".into())
}

pub fn force_agent_collect(install_dir: &Path) {
    let _ = force_agent_collect_wait(install_dir, 360);
}

pub fn force_agent_collect_wait(install_dir: &Path, timeout_secs: u64) -> Result<(), String> {
    let agent_exe = install_dir.join("belarc-agent.exe");
    if !agent_exe.is_file() {
        return Err("belarc-agent.exe nao encontrado".into());
    }
    let mut child = Command::new(&agent_exe)
        .args(["collect", "t1", "--force"])
        .spawn()
        .map_err(|e| format!("falha ao iniciar coleta: {e}"))?;
    let start = std::time::Instant::now();
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            if !status.success() {
                return Err(format!(
                    "coleta terminou com erro (exit {})",
                    status.code().unwrap_or(-1)
                ));
            }
            return Ok(());
        }
        if start.elapsed().as_secs() >= timeout_secs {
            let _ = child.kill();
            return Err(format!("coleta demorou mais de {timeout_secs}s"));
        }
        thread::sleep(Duration::from_secs(2));
    }
}

pub fn verify_service_running() -> bool {
    if let Ok(output) = run_cmd("sc", &["query", AGENT_SERVICE]) {
        return cmd_output_text(&output).to_uppercase().contains("RUNNING");
    }
    false
}

pub async fn verify_machine_on_server(server_url: &str, hostname: &str, wait_secs: u64) -> bool {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .unwrap();
    let url = format!("{}/api/machines", server_url.trim_end_matches('/'));
    for _ in 0..wait_secs.max(1) / 2 {
        if let Ok(resp) = client.get(&url).send().await {
            if resp.status().is_success() {
                if let Ok(list) = resp.json::<Vec<serde_json::Value>>().await {
                    if list.iter().any(|m| {
                        m.get("hostname")
                            .and_then(|h| h.as_str())
                            .map(|h| h.eq_ignore_ascii_case(hostname))
                            .unwrap_or(false)
                    }) {
                        return true;
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    false
}

pub fn remove_legacy_http_agent() {
    nas_pc_log("remove_legacy_http_agent: inicio");
    let install_dir = PathBuf::from(INSTALL_DIR);
    let agent_exe = install_dir.join("belarc-agent.exe");
    let _ = run_cmd("sc", &["stop", AGENT_SERVICE]);
    thread::sleep(Duration::from_secs(1));
    if agent_exe.is_file() {
        nas_pc_log("remove_legacy: belarc-agent uninstall");
        let mut child = Command::new(&agent_exe).arg("uninstall").spawn().ok();
        if let Some(mut c) = child.take() {
            for _ in 0..15 {
                if c.try_wait().ok().flatten().is_some() {
                    break;
                }
                thread::sleep(Duration::from_secs(1));
            }
            let _ = c.kill();
        }
        thread::sleep(Duration::from_secs(1));
    }
    let _ = run_cmd("sc", &["stop", AGENT_SERVICE]);
    let _ = run_cmd("sc", &["delete", AGENT_SERVICE]);
    remove_ticket_portal_shortcut();
    let _ = run_cmd("taskkill", &["/F", "/IM", "belarc-agent.exe"]);
    if agent_exe.is_file() {
        let _ = fs::remove_file(&agent_exe);
    }
    let ps = r#"
foreach ($t in @('BelarcInventoryRepair','BelarcForceCollect')) {
  Unregister-ScheduledTask -TaskName $t -Confirm:$false -ErrorAction SilentlyContinue
}
"#;
    let _ = run_powershell_hidden(ps);
    for name in ["config.toml", "install-info.json"] {
        let p = Path::new(AGENT_DATA).join(name);
        if p.is_file() {
            let _ = fs::remove_file(&p);
        }
    }
    nas_pc_log("remove_legacy_http_agent: ok");
}

pub fn run_uninstall_core(include_server: bool) {
    if include_server {
        let _ = run_cmd("schtasks", &["/End", "/TN", SERVER_TASK]);
        let _ = run_cmd("schtasks", &["/Delete", "/F", "/TN", SERVER_TASK]);
        let _ = run_cmd("schtasks", &["/End", "/TN", REPAIR_TASK]);
        let _ = run_cmd("schtasks", &["/Delete", "/F", "/TN", REPAIR_TASK]);
    }
    let _ = run_cmd("sc", &["stop", AGENT_SERVICE]);
    let _ = run_cmd("sc", &["delete", AGENT_SERVICE]);
}

pub(crate) fn chrono_lite_now() -> String {
    use std::time::SystemTime;
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", dur.as_secs())
}
