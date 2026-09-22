use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::is_admin;

/// Garante Admin (UAC) e exe em disco local (nao UNC).
/// Retorna Some(path) se reexecutou — o processo atual deve sair.
pub fn ensure_ready_exe(exe_name: &str) -> Option<PathBuf> {
    if let Some(local) = ensure_local_exe(exe_name) {
        return Some(local);
    }
    if !is_admin() {
        if let Some(relaunched) = request_elevation() {
            return Some(relaunched);
        }
    }
    None
}

fn exe_path() -> PathBuf {
    env::current_exe().unwrap_or_else(|_| PathBuf::from(exe_name_default()))
}

fn exe_name_default() -> &'static str {
    "BelarcPC.exe"
}

fn request_elevation() -> Option<PathBuf> {
    let exe = exe_path();
    let args: Vec<String> = env::args().skip(1).collect();
    let exe_esc = exe.to_string_lossy().replace('\'', "''");

    // ArgumentList '' quebra o exe elevado (argv vazio invalido)
    let ps = if args.is_empty() {
        format!(r#"Start-Process -FilePath '{exe_esc}' -Verb RunAs -Wait"#)
    } else {
        let arg_str = args
            .iter()
            .map(|a| {
                if a.contains(' ') {
                    format!("'{}'", a.replace('\'', "''"))
                } else {
                    a.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(r#"Start-Process -FilePath '{exe_esc}' -ArgumentList {arg_str} -Verb RunAs -Wait"#)
    };

    eprintln!();
    eprintln!("Solicitando permissoes de Administrador...");
    eprintln!("(A janela pode fechar aqui — continue na janela elevada)");
    let ok = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if ok {
        Some(exe)
    } else {
        eprintln!("ERRO: permissao de administrador negada.");
        crate::pause("");
        None
    }
}

/// Se o exe esta em \\NAS\..., copia para %TEMP%\Belarc\ e reexecuta.
fn ensure_local_exe(exe_name: &str) -> Option<PathBuf> {
    let exe = exe_path();
    let s = exe.to_string_lossy();
    if !s.starts_with("\\\\") {
        return None;
    }

    eprintln!("Instalador no NAS — copiando para disco local...");
    let dest_dir = env::temp_dir().join("Belarc");
    let _ = fs::create_dir_all(&dest_dir);
    let dest = dest_dir.join(exe_name);
    if fs::copy(&exe, &dest).is_err() {
        eprintln!(
            "ERRO: nao foi possivel copiar do NAS para {}",
            dest.display()
        );
        eprintln!("Copie o .exe para C:\\Temp e execute de la.");
        return Some(dest);
    }

    let args: Vec<String> = env::args().skip(1).collect();
    let dest_esc = dest.to_string_lossy().replace('\'', "''");
    let ps = if args.is_empty() {
        format!(r#"Start-Process -FilePath '{dest_esc}' -Verb RunAs -Wait"#)
    } else {
        let arg_str = args
            .iter()
            .map(|a| format!("'{}'", a.replace('\'', "''")))
            .collect::<Vec<_>>()
            .join(",");
        format!(r#"Start-Process -FilePath '{dest_esc}' -ArgumentList {arg_str} -Verb RunAs -Wait"#)
    };
    eprintln!("Executando copia local com elevacao...");
    let _ = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps])
        .status();
    Some(dest)
}
