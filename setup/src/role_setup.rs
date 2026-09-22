#![allow(clippy::items_after_test_module)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::nas::deploy_http_from_pc_bundle;
use crate::{
    create_token, force_agent_collect_wait, install_agent_service, is_admin, localhost_server_url,
    wait_server, AGENT_DATA, INSTALL_DIR,
};

const V2_DIR: &str = r"C:\Program Files\BelarcInventory\ChamadosServicosTI-v2";
const V2_EXE: &str = "ChamadosServicosTI-v2.exe";

pub fn run_client_setup(
    rt: &tokio::runtime::Runtime,
    pc_bundle: &'static [u8],
    ticket_v2: &'static [u8],
    ticket_icon: &'static [u8],
    server_url: &str,
) {
    run_role_setup(
        rt,
        pc_bundle,
        ticket_v2,
        ticket_icon,
        server_url,
        Role::Client,
    )
}

pub fn run_ti_setup(
    rt: &tokio::runtime::Runtime,
    pc_bundle: &'static [u8],
    ticket_v2: &'static [u8],
    ticket_icon: &'static [u8],
    server_url: &str,
) {
    run_role_setup(rt, pc_bundle, ticket_v2, ticket_icon, server_url, Role::Ti)
}

#[derive(Clone, Copy)]
enum Role {
    Client,
    Ti,
}

impl Role {
    fn title(self) -> &'static str {
        match self {
            Self::Client => "Belarc Cliente Setup",
            Self::Ti => "Belarc TI Setup",
        }
    }
}

fn run_role_setup(
    rt: &tokio::runtime::Runtime,
    pc_bundle: &'static [u8],
    ticket_v2: &'static [u8],
    ticket_icon: &'static [u8],
    server_url: &str,
    role: Role,
) {
    println!("========================================");
    println!("  {} — homologação controlada", role.title());
    println!("  Servidor: {server_url}");
    println!("========================================");

    if !is_admin() {
        eprintln!("ERRO: execute como Administrador.");
        crate::pause("");
        std::process::exit(1);
    }
    if ticket_v2.len() < 100_000 {
        eprintln!("ERRO: o cliente novo não está dentro deste setup. Gere o pacote novamente.");
        crate::pause("");
        std::process::exit(1);
    }
    let normalized_url = server_url.trim_end_matches('/');
    println!("[1/4] Verificando servidor...");
    if !rt.block_on(wait_server(normalized_url, 8)) {
        eprintln!("ERRO: servidor não respondeu em {normalized_url}/api/health.");
        eprintln!("Nenhuma instalação foi iniciada.");
        crate::pause("");
        std::process::exit(1);
    }

    let local_server = is_local_server_pc(rt);
    if local_server {
        println!(
            "  Função detectada: servidor + TI (agente local preservado; painel em {normalized_url})"
        );
    } else {
        println!("  Função detectada: estação conectada ao servidor {normalized_url}");
    }

    println!("[2/4] Verificando agente e identificação deste PC...");
    let effective_url = match ensure_agent(rt, pc_bundle, normalized_url, local_server) {
        Ok(url) => url,
        Err(err) => {
            eprintln!("ERRO ao preparar agente: {err}");
            crate::pause("");
            std::process::exit(1);
        }
    };

    println!("[3/4] Instalando cliente de chamados v2 sem remover o legado...");
    let (ticket_exe, ticket_icon_path) = match install_ticket_client(ticket_v2, ticket_icon) {
        Ok(path) => path,
        Err(err) => {
            eprintln!("ERRO ao instalar cliente: {err}");
            crate::pause("");
            std::process::exit(1);
        }
    };

    println!("[4/4] Criando atalhos na Área de Trabalho Pública...");
    if let Err(err) = create_shortcuts(&ticket_exe, &ticket_icon_path, &effective_url, role) {
        eprintln!("ERRO ao criar atalhos: {err}");
        crate::pause("");
        std::process::exit(1);
    }
    write_manifest(&ticket_exe, &effective_url, role);

    println!();
    println!("=== INSTALADO PARA HOMOLOGAÇÃO ===");
    println!("Cliente novo: {}", ticket_exe.display());
    match role {
        Role::Client => println!("Atalho: Chamados Serviços TI"),
        Role::Ti => {
            println!("Atalhos: Belarc Inventory — Painel TI e Chamados Serviços TI");
            println!(
                "O painel concentra Inventário, Gestão TI, Diretório, Chamados e Dashboard BI."
            );
        }
    }
    println!("O executável/atalho legado não foi removido.");
    crate::pause("Concluído.");
}

fn ensure_agent(
    rt: &tokio::runtime::Runtime,
    pc_bundle: &[u8],
    server_url: &str,
    local_server: bool,
) -> Result<String, String> {
    let config = Path::new(AGENT_DATA).join("config.toml");
    if config.is_file() {
        // Atualização: preservar token, identidade, serviço e agenda atuais.
        // O cliente v2 usa esta mesma configuração sem regravá-la.
        let text = fs::read_to_string(&config).map_err(|e| e.to_string())?;
        if let Some(configured) = config_value(&text, "server_url") {
            if !configured.eq_ignore_ascii_case(server_url) {
                if local_server && is_loopback_url(&configured) {
                    println!(
                        "  OK: este é o servidor. O agente continuará usando {configured} e os atalhos usarão {server_url}."
                    );
                    return Ok(server_url.to_string());
                }
                return Err(format!(
                    "este PC já está configurado para {configured}, diferente do servidor informado {server_url}, e não foi detectado um servidor local ativo. Nenhuma configuração foi alterada. Use --server-url {configured} ou valide o ambiente antes de trocar de servidor."
                ));
            }
            return Ok(server_url.to_string());
        }

        // Configurações antigas podem conter BOM UTF-8 ou ter sido copiadas
        // apenas junto do servidor. Não descartamos o token: fazemos um
        // backup do arquivo e reconstruímos somente a configuração do agente.
        // Isso permite que o setup de TI seja executado no próprio servidor
        // sem apagar identidade, coleta ou dados do inventário.
        let backup = config.with_file_name(format!(
            "config.toml.pre-ti-setup-{}",
            crate::chrono_lite_now()
        ));
        fs::copy(&config, &backup).map_err(|e| {
            format!("configuração existente não possui server_url válida e o backup falhou: {e}")
        })?;
        let token = config_value(&text, "agent_token")
            .filter(|value| !value.trim().is_empty())
            .map(Ok)
            .unwrap_or_else(|| rt.block_on(create_token(server_url)))?;
        let install_dir = deploy_http_from_pc_bundle(pc_bundle)?;
        let agent_exe = install_dir.join("belarc-agent.exe");
        let agent_url = agent_server_url(server_url, local_server);
        install_agent_service(&agent_exe, &agent_url, &token)?;
        let _ = force_agent_collect_wait(&install_dir, 360);
        return Ok(server_url.to_string());
    }
    let install_dir = deploy_http_from_pc_bundle(pc_bundle)?;
    let token = rt.block_on(create_token(server_url))?;
    let agent_exe = install_dir.join("belarc-agent.exe");
    let agent_url = agent_server_url(server_url, local_server);
    install_agent_service(&agent_exe, &agent_url, &token)?;
    let _ = force_agent_collect_wait(&install_dir, 360);
    Ok(server_url.to_string())
}

fn is_local_server_pc(rt: &tokio::runtime::Runtime) -> bool {
    let server_exe = Path::new(INSTALL_DIR).join("belarc-server.exe");
    server_exe.is_file() && rt.block_on(wait_server(&localhost_server_url(), 2))
}

fn agent_server_url(server_url: &str, local_server: bool) -> String {
    if local_server {
        localhost_server_url().trim_end_matches('/').to_string()
    } else {
        server_url.to_string()
    }
}

fn is_loopback_url(value: &str) -> bool {
    let lower = value.trim().trim_end_matches('/').to_ascii_lowercase();
    [
        "http://127.0.0.1",
        "https://127.0.0.1",
        "http://localhost",
        "https://localhost",
        "http://[::1]",
        "https://[::1]",
    ]
    .iter()
    .any(|prefix| lower == *prefix || lower.starts_with(&format!("{prefix}:")))
}

fn config_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let line = line.trim().trim_start_matches('\u{feff}');
        let (name, value) = line.split_once('=')?;
        if name.trim() != key {
            return None;
        }
        let value = value
            .split('#')
            .next()
            .unwrap_or(value)
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .trim_end_matches('/')
            .trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::{config_value, is_loopback_url};

    #[test]
    fn reads_server_url_with_utf8_bom() {
        let text = "\u{feff}server_url = \"http://127.0.0.1\"\nagent_token = \"abc\"\n";
        assert_eq!(
            config_value(text, "server_url").as_deref(),
            Some("http://127.0.0.1")
        );
        assert_eq!(config_value(text, "agent_token").as_deref(), Some("abc"));
    }

    #[test]
    fn identifies_supported_loopback_urls() {
        assert!(is_loopback_url("http://127.0.0.1"));
        assert!(is_loopback_url("http://localhost:8080/"));
        assert!(is_loopback_url("http://[::1]:8080"));
        assert!(!is_loopback_url("http://192.0.2.10"));
    }
}

fn install_ticket_client(bytes: &[u8], icon_bytes: &[u8]) -> Result<(PathBuf, PathBuf), String> {
    let dir = PathBuf::from(V2_DIR);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let target = dir.join(V2_EXE);
    if target.is_file() {
        let backup = dir.join("ChamadosServicosTI-v2.anterior.exe");
        let _ = fs::copy(&target, backup);
    }
    let staged = dir.join("ChamadosServicosTI-v2.novo.exe");
    fs::write(&staged, bytes).map_err(|e| e.to_string())?;
    fs::rename(&staged, &target)
        .or_else(|_| {
            let _ = fs::remove_file(&target);
            fs::rename(&staged, &target)
        })
        .map_err(|e| e.to_string())?;
    let icon = dir.join("BelarcChamadosTI.ico");
    if icon_bytes.len() > 32 {
        fs::write(&icon, icon_bytes).map_err(|e| e.to_string())?;
    }
    Ok((target, icon))
}

fn create_shortcuts(
    ticket_exe: &Path,
    ticket_icon: &Path,
    server_url: &str,
    role: Role,
) -> Result<(), String> {
    let ticket = ticket_exe.to_string_lossy().replace('\'', "''");
    let workdir = ticket_exe
        .parent()
        .unwrap_or_else(|| Path::new(INSTALL_DIR))
        .to_string_lossy()
        .replace('\'', "''");
    let url = server_url.replace('\'', "''");
    let icon = ticket_icon.to_string_lossy().replace('\'', "''");
    let ps = match role {
        Role::Client => format!(
            r#"
$shell = New-Object -ComObject WScript.Shell
$desktops = @([Environment]::GetFolderPath('CommonDesktopDirectory'), [Environment]::GetFolderPath('Desktop')) | Where-Object {{ $_ -and (Test-Path -LiteralPath $_) }} | Select-Object -Unique
foreach ($desktop in $desktops) {{
  $shortcut = $shell.CreateShortcut((Join-Path $desktop 'Chamados Serviços TI.lnk'))
  $shortcut.TargetPath = '{ticket}'
  $shortcut.WorkingDirectory = '{workdir}'
  $shortcut.IconLocation = '{icon},0'
  $shortcut.Description = 'Abrir chamados deste computador'
  $shortcut.Save()
}}
"#
        ),
        Role::Ti => format!(
            r#"
$shell = New-Object -ComObject WScript.Shell
$desktops = @([Environment]::GetFolderPath('CommonDesktopDirectory'), [Environment]::GetFolderPath('Desktop')) | Where-Object {{ $_ -and (Test-Path -LiteralPath $_) }} | Select-Object -Unique
foreach ($desktop in $desktops) {{
  $panel = $shell.CreateShortcut((Join-Path $desktop 'Belarc Inventory — Painel TI.lnk'))
  $panel.TargetPath = "$env:WINDIR\explorer.exe"
  $panel.Arguments = '{url}'
  $panel.Description = 'Inventário, Gestão TI, Diretório, Chamados e Dashboard BI'
  $panel.Save()
  $tickets = $shell.CreateShortcut((Join-Path $desktop 'Chamados Serviços TI.lnk'))
  $tickets.TargetPath = '{ticket}'
  $tickets.WorkingDirectory = '{workdir}'
  $tickets.IconLocation = '{icon},0'
  $tickets.Description = 'Abrir chamados deste computador'
  $tickets.Save()
}}
"#
        ),
    };
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps])
        .output()
        .map_err(|e| format!("criação de atalhos falhou: {e}"))?;
    if !output.status.success() {
        return Err(crate::cmd_output_text(&output));
    }
    Ok(())
}

fn write_manifest(ticket_exe: &Path, server_url: &str, role: Role) {
    let root = Path::new(AGENT_DATA).join("ChamadosServicosTI-v2");
    let _ = fs::create_dir_all(&root);
    let sha256 = Command::new("certutil")
        .args(["-hashfile", &ticket_exe.to_string_lossy(), "SHA256"])
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .nth(1)
                .unwrap_or_default()
                .trim()
                .to_string()
        })
        .unwrap_or_default();
    let role_name = match role {
        Role::Client => "cliente",
        Role::Ti => "ti",
    };
    let body = serde_json::json!({
        "role": role_name,
        "server_url": server_url,
        "ticket_executable": ticket_exe,
        "ticket_sha256": sha256,
        "legacy_preserved": true,
    });
    let _ = fs::write(root.join("setup-manifest.json"), body.to_string());
}
