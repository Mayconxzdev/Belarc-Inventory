use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{
    cmd_output_text, copy_dir_all, copy_file_if_exists, extract_zip_to_dir, run_cmd,
    run_powershell_hidden, AGENT_DATA, INSTALL_DIR, NAS_EXPORT_DIR, NAS_IMPORT_DIR, NAS_TASK_FULL,
    NAS_TASK_FULL_BOOT, NAS_TASK_IMPORT, NAS_TASK_PRESENCE,
};

pub fn deploy_pc_bundle(zip_bytes: &[u8]) -> Result<PathBuf, String> {
    let staging = Path::new(AGENT_DATA).join("_nas_pc_staging");
    let _ = fs::remove_dir_all(&staging);
    extract_zip_to_dir(&staging, zip_bytes)?;

    let install_dir = PathBuf::from(INSTALL_DIR);
    let collectors_dst = install_dir.join("collectors");
    let export_dir = PathBuf::from(NAS_EXPORT_DIR);
    let export_config = export_dir.join("config");

    fs::create_dir_all(&collectors_dst).map_err(|e| e.to_string())?;
    fs::create_dir_all(&export_config).map_err(|e| e.to_string())?;

    let collectors_src = staging.join("collectors");
    if collectors_src.is_dir() {
        copy_dir_all(&collectors_src, &collectors_dst)?;
    }

    let profile_src = staging.join("config").join("company-profile.json");
    if profile_src.is_file() {
        copy_file_if_exists(&profile_src, &collectors_dst.join("_company-profile.json"))?;
        let cfg_dir = install_dir.join("config");
        fs::create_dir_all(&cfg_dir).ok();
        copy_file_if_exists(&profile_src, &cfg_dir.join("company-profile.json"))?;
    }

    for (src_name, dst) in [
        (
            staging.join("config").join("belarc-deploy.ps1"),
            export_config.join("belarc-deploy.ps1"),
        ),
        (
            staging.join("config").join("nas-sync.ps1"),
            export_config.join("nas-sync.ps1"),
        ),
        (
            staging.join("exportar-para-nas.ps1"),
            export_dir.join("exportar-para-nas.ps1"),
        ),
        (
            staging.join("exportar-presence-nas.ps1"),
            export_dir.join("exportar-presence-nas.ps1"),
        ),
    ] {
        copy_file_if_exists(&src_name, &dst)?;
    }

    let _ = fs::remove_dir_all(&staging);
    Ok(export_dir)
}

pub fn deploy_http_from_pc_bundle(zip_bytes: &[u8]) -> Result<PathBuf, String> {
    let staging = Path::new(AGENT_DATA).join("_pc_http_staging");
    let _ = fs::remove_dir_all(&staging);
    extract_zip_to_dir(&staging, zip_bytes)?;

    let install_dir = PathBuf::from(INSTALL_DIR);
    fs::create_dir_all(&install_dir).map_err(|e| e.to_string())?;

    let agent_src = staging.join("belarc-agent.exe");
    if agent_src.is_file() {
        copy_file_if_exists(&agent_src, &install_dir.join("belarc-agent.exe"))?;
    }

    let collectors_src = staging.join("collectors");
    if collectors_src.is_dir() {
        copy_dir_all(&collectors_src, &install_dir.join("collectors"))?;
    }

    let profile_src = staging.join("config").join("company-profile.json");
    if profile_src.is_file() {
        let cfg = install_dir.join("config");
        fs::create_dir_all(&cfg).ok();
        copy_file_if_exists(&profile_src, &cfg.join("company-profile.json"))?;
        copy_file_if_exists(
            &profile_src,
            &install_dir.join("collectors").join("_company-profile.json"),
        )?;
    }

    let _ = fs::remove_dir_all(&staging);
    Ok(install_dir)
}

pub fn deploy_server_nas_bundle(zip_bytes: &[u8]) -> Result<PathBuf, String> {
    let staging = Path::new(AGENT_DATA).join("_nas_srv_staging");
    let _ = fs::remove_dir_all(&staging);
    extract_zip_to_dir(&staging, zip_bytes)?;

    let import_dir = PathBuf::from(NAS_IMPORT_DIR);
    let import_config = import_dir.join("config");
    fs::create_dir_all(&import_config).map_err(|e| e.to_string())?;

    copy_file_if_exists(
        &staging.join("config").join("belarc-deploy.ps1"),
        &import_config.join("belarc-deploy.ps1"),
    )?;
    copy_file_if_exists(
        &staging.join("config").join("nas-sync.ps1"),
        &import_config.join("nas-sync.ps1"),
    )?;
    copy_file_if_exists(
        &staging.join("importar-do-nas.ps1"),
        &import_dir.join("importar-do-nas.ps1"),
    )?;
    copy_file_if_exists(
        &staging.join("indexar-frota-nas.ps1"),
        &import_dir.join("indexar-frota-nas.ps1"),
    )?;
    copy_file_if_exists(
        &staging.join("servidor-nas-sync.ps1"),
        &import_dir.join("servidor-nas-sync.ps1"),
    )?;

    let portal_src = staging.join("portal").join("web");
    if portal_src.is_dir() {
        let _ = deploy_portal_to_nas(&portal_src);
    }

    let _ = fs::remove_dir_all(&staging);
    Ok(import_dir.join("servidor-nas-sync.ps1"))
}

fn resolve_nas_portal_web_path() -> Result<PathBuf, String> {
    let ps = r#"
$candidates = @(
  '\\FILE-SHARE\Portal\Belarc\web',
  '\\192.0.2.11\Portal\Belarc\web'
)
foreach ($p in $candidates) {
  try {
    if (Test-Path -LiteralPath $p -ErrorAction Stop) {
      Write-Output $p
      exit 0
    }
  } catch {}
}
$root = $null
foreach ($pcs in @('\\FILE-SHARE\Portal\Belarc\PCs','\\192.0.2.11\Portal\Belarc\PCs')) {
  if (Test-Path -LiteralPath $pcs) { $root = Split-Path $pcs -Parent; break }
}
if ($root) {
  $w = Join-Path $root 'web'
  New-Item -ItemType Directory -Force -Path $w | Out-Null
  Write-Output $w
}
"#;
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", ps])
        .output()
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        Err("portal NAS inacessivel".into())
    } else {
        Ok(PathBuf::from(text))
    }
}

pub fn deploy_portal_to_nas(staging_portal: &Path) -> Result<(), String> {
    if !staging_portal.is_dir() {
        return Ok(());
    }
    let dest = resolve_nas_portal_web_path()?;
    copy_dir_all(staging_portal, &dest)?;
    println!("  OK: portal web em {}", dest.display());
    Ok(())
}

pub fn test_nas_share_access() -> bool {
    let ps = r#"
$candidates = @(
  '\\FILE-SHARE\Portal\Belarc\PCs',
  '\\192.0.2.11\Portal\Belarc\PCs',
  '\\FILE-SHARE\Public\__arquivos-ti\Infos - PCs\BELARC_PC',
  '\\192.0.2.11\Public\__arquivos-ti\Infos - PCs\BELARC_PC'
)
foreach ($p in $candidates) {
  try {
    if (Test-Path -LiteralPath $p -ErrorAction Stop) { exit 0 }
  } catch {}
}
exit 1
"#;
    run_cmd("powershell.exe", &["-NoProfile", "-Command", ps])
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn nas_task_principal_user() -> String {
    let domain = std::env::var("USERDOMAIN").unwrap_or_else(|_| ".".into());
    let user = std::env::var("USERNAME").unwrap_or_else(|_| "SYSTEM".into());
    format!("{domain}\\{user}")
}

pub fn register_pc_tasks(export_dir: &Path) -> Result<(), String> {
    let task_user = nas_task_principal_user().replace('\'', "''");
    let presence = export_dir
        .join("exportar-presence-nas.ps1")
        .to_string_lossy()
        .replace('\'', "''");
    let full = export_dir
        .join("exportar-para-nas.ps1")
        .to_string_lossy()
        .replace('\'', "''");
    let ps = format!(
        r#"
$presenceArgs = '-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File ''{presence}'' -Quiet'
$fullArgs = '-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File ''{full}'' -Quiet'
$principal = New-ScheduledTaskPrincipal -UserId '{task_user}' -LogonType Interactive -RunLevel Highest
$settingsPresence = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -Hidden -ExecutionTimeLimit (New-TimeSpan -Minutes 10)
$settingsFull = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -Hidden -ExecutionTimeLimit (New-TimeSpan -Hours 2)
foreach ($t in @('BelarcInventoryNas','BelarcInventoryNasBoot','{NAS_TASK_PRESENCE}','{NAS_TASK_FULL}','{NAS_TASK_FULL_BOOT}')) {{
  Unregister-ScheduledTask -TaskName $t -Confirm:$false -ErrorAction SilentlyContinue
}}
$actionP = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument $presenceArgs
$triggerP = New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes(1) -RepetitionInterval (New-TimeSpan -Minutes 5) -RepetitionDuration (New-TimeSpan -Days 3650)
Register-ScheduledTask -TaskName '{NAS_TASK_PRESENCE}' -Action $actionP -Trigger $triggerP -Principal $principal -Settings $settingsPresence -Force | Out-Null
$actionF = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument $fullArgs
$triggerF = New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes(3) -RepetitionInterval (New-TimeSpan -Hours 6) -RepetitionDuration (New-TimeSpan -Days 3650)
$triggerB = New-ScheduledTaskTrigger -AtStartup
$triggerB.Delay = 'PT3M'
Register-ScheduledTask -TaskName '{NAS_TASK_FULL}' -Action $actionF -Trigger $triggerF -Principal $principal -Settings $settingsFull -Force | Out-Null
Register-ScheduledTask -TaskName '{NAS_TASK_FULL_BOOT}' -Action $actionF -Trigger $triggerB -Principal $principal -Settings $settingsFull -Force | Out-Null
Start-ScheduledTask -TaskName '{NAS_TASK_PRESENCE}' -ErrorAction SilentlyContinue
Start-ScheduledTask -TaskName '{NAS_TASK_FULL}' -ErrorAction SilentlyContinue
"#
    );
    run_powershell_hidden(&ps)
}

pub fn register_import_task(import_script: &Path) -> Result<(), String> {
    let script = import_script.to_string_lossy().replace('\'', "''");
    let ps = format!(
        r#"
$taskName = '{NAS_TASK_IMPORT}'
$args = '-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File ''{script}'' -Quiet'
$action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument $args
$principal = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -LogonType ServiceAccount -RunLevel Highest
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -Hidden -ExecutionTimeLimit (New-TimeSpan -Hours 1)
Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue
$trigger = New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes(1) -RepetitionInterval (New-TimeSpan -Minutes 3) -RepetitionDuration (New-TimeSpan -Days 3650)
Register-ScheduledTask -TaskName $taskName -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null
Start-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
"#
    );
    run_powershell_hidden(&ps)
}

pub fn run_script_background(script: &Path) {
    let script = script.to_string_lossy().replace('\'', "''");
    let ps = format!(
        "Start-Process powershell.exe -ArgumentList '-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File ''{script}'' -Quiet' -WindowStyle Hidden"
    );
    let _ = run_powershell_hidden(&ps);
}

pub fn run_script_wait(script: &Path) -> Result<String, String> {
    let script_str = script.to_string_lossy();
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            &script_str,
            "-Quiet",
        ])
        .output()
        .map_err(|e| format!("powershell falhou: {e}"))?;
    let text = cmd_output_text(&output);
    if !output.status.success() {
        return Err(text);
    }
    Ok(text)
}

pub fn run_import_once(import_script: &Path) -> Result<(), String> {
    let script = import_script.to_string_lossy();
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", &script])
        .output()
        .map_err(|e| format!("import falhou: {e}"))?;
    if !output.status.success() {
        return Err(cmd_output_text(&output));
    }
    Ok(())
}

pub fn verify_presence_on_share(hostname: &str) -> bool {
    let host = hostname.replace('\'', "''");
    let ps = format!(
        r#"
. 'C:\ProgramData\BelarcInventory\nas-export\config\belarc-deploy.ps1'
$folder = Join-Path (Get-BelarcNasRoot) '{host}'
$p = Join-Path $folder 'presence.json'
if ((Test-Path -LiteralPath $p)) {{ exit 0 }} else {{ exit 1 }}
"#
    );
    run_cmd(
        "powershell.exe",
        &["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps],
    )
    .map(|o| o.status.success())
    .unwrap_or(false)
}

pub fn uninstall_pc_tasks() {
    let ps = r#"
foreach ($t in @('BelarcInventoryNas','BelarcInventoryNasBoot','BelarcInventoryNasPresence','BelarcInventoryNasFull','BelarcInventoryNasFullBoot')) {
  Unregister-ScheduledTask -TaskName $t -Confirm:$false -ErrorAction SilentlyContinue
}
"#;
    let _ = run_powershell_hidden(ps);
}

pub fn uninstall_import_task() {
    let ps = format!(
        "Unregister-ScheduledTask -TaskName '{NAS_TASK_IMPORT}' -Confirm:$false -ErrorAction SilentlyContinue"
    );
    let _ = run_powershell_hidden(&ps);
}
