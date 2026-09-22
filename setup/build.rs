use std::env;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

fn add_path<W: Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    base: &Path,
    zip_name: &str,
) -> std::io::Result<()> {
    if base.is_file() {
        zip.start_file(zip_name, zip::write::SimpleFileOptions::default())?;
        let mut f = File::open(base)?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        zip.write_all(&buf)?;
        return Ok(());
    }
    for entry in std::fs::read_dir(base)? {
        let entry = entry?;
        let path = entry.path();
        let fname = entry.file_name().to_string_lossy().to_string();
        let child_zip = if zip_name.is_empty() {
            fname.clone()
        } else {
            format!("{zip_name}/{fname}")
        };
        if path.is_dir() {
            add_path(zip, &path, &child_zip)?;
        } else {
            zip.start_file(&child_zip, zip::write::SimpleFileOptions::default())?;
            let mut f = File::open(&path)?;
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)?;
            zip.write_all(&buf)?;
        }
    }
    Ok(())
}

fn build_zip(path: &Path, add: impl FnOnce(&mut zip::ZipWriter<File>) -> std::io::Result<()>) {
    if let Ok(file) = File::create(path) {
        let mut zip = zip::ZipWriter::new(file);
        if add(&mut zip).is_ok() {
            let _ = zip.finish();
        }
    }
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace = manifest_dir.parent().unwrap();
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let pc_bundle = out_dir.join("pc-bundle.zip");
    let servidor_bundle = out_dir.join("servidor-bundle.zip");
    let ticket_v2_bundle = out_dir.join("chamados-v2.exe");
    let ticket_icon_bundle = out_dir.join("chamados-v2.ico");

    let profile = env::var("PROFILE").unwrap_or_else(|_| "release".into());
    let target_dir = workspace.join("target").join(&profile);

    let server_exe = target_dir.join("belarc-server.exe");
    let agent_exe = target_dir.join("belarc-agent.exe");
    let web_dir = workspace.join("server").join("web");
    let collectors_dir = workspace.join("agent").join("collectors");
    let company_profile = workspace.join("config").join("company-profile.json");
    let production_profile = workspace
        .join("config")
        .join("company-profile.production.json");
    let nas_res = manifest_dir.join("resources").join("nas");
    let portal_web = workspace.join("portal").join("web");
    let ticket_v2_exe = env::var("BELARC_TICKET_V2_EXE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            workspace
                .join("desktop")
                .join("chamados-servicos-ti-v2")
                .join("build")
                .join("ChamadosServicosTI-v2.exe")
        });
    let ticket_icon = workspace
        .join("bk")
        .join("RESULTADO-HOMOLOGACAO-20260914-111110")
        .join("web-publicada")
        .join("favicon.ico");

    let repair_scripts = [
        "reparo-automatico.ps1",
        "agendar-reparo-automatico.ps1",
        "manutencao-pc.ps1",
        "executar-manutencao.ps1",
    ];

    println!("cargo:rerun-if-changed={}", server_exe.display());
    println!("cargo:rerun-if-changed={}", agent_exe.display());
    println!("cargo:rerun-if-changed={}", nas_res.display());
    println!("cargo:rerun-if-changed={}", ticket_v2_exe.display());
    println!("cargo:rerun-if-changed={}", ticket_icon.display());

    let profile_for_bundle = if production_profile.is_file() {
        &production_profile
    } else {
        &company_profile
    };

    if !agent_exe.exists() {
        let _ = std::fs::write(&pc_bundle, []);
        let _ = std::fs::write(&servidor_bundle, []);
        let _ = std::fs::write(&ticket_v2_bundle, []);
        let _ = std::fs::write(&ticket_icon_bundle, []);
        emit_env(
            &pc_bundle,
            &servidor_bundle,
            &ticket_v2_bundle,
            &ticket_icon_bundle,
        );
        println!("cargo:warning=Compile belarc-server and belarc-agent before belarc-setup");
        return;
    }

    // BelarcPC.exe bundle: NAS export + optional HTTP agent
    build_zip(&pc_bundle, |zip| {
        add_path(zip, &collectors_dir, "collectors")?;
        if profile_for_bundle.is_file() {
            add_path(zip, profile_for_bundle, "config/company-profile.json")?;
        }
        for name in [
            "belarc-deploy.ps1",
            "nas-sync.ps1",
            "exportar-presence-nas.ps1",
            "exportar-para-nas.ps1",
        ] {
            let p = nas_res.join(name);
            if p.is_file() {
                if name.ends_with("deploy.ps1") || name == "nas-sync.ps1" {
                    add_path(zip, &p, &format!("config/{name}"))?;
                } else {
                    add_path(zip, &p, name)?;
                }
            }
        }
        if agent_exe.is_file() {
            add_path(zip, &agent_exe, "belarc-agent.exe")?;
        }
        Ok(())
    });

    // BelarcServidor.exe bundle: HTTP server + NAS sync + portal
    build_zip(&servidor_bundle, |zip| {
        if server_exe.is_file() {
            add_path(zip, &server_exe, "belarc-server.exe")?;
        }
        add_path(zip, &agent_exe, "belarc-agent.exe")?;
        add_path(zip, &web_dir, "web")?;
        add_path(zip, &collectors_dir, "collectors")?;
        if profile_for_bundle.is_file() {
            add_path(zip, profile_for_bundle, "config/company-profile.json")?;
        }
        for name in [
            "belarc-deploy.ps1",
            "nas-sync.ps1",
            "importar-do-nas.ps1",
            "indexar-frota-nas.ps1",
            "servidor-nas-sync.ps1",
        ] {
            let p = nas_res.join(name);
            if p.is_file() {
                if name.ends_with("deploy.ps1") || name == "nas-sync.ps1" {
                    add_path(zip, &p, &format!("config/{name}"))?;
                } else {
                    add_path(zip, &p, name)?;
                }
            }
        }
        if portal_web.is_dir() {
            add_path(zip, &portal_web, "portal/web")?;
        }
        for script in repair_scripts {
            let p = workspace.join(script);
            if p.is_file() {
                add_path(zip, &p, script)?;
            }
        }
        Ok(())
    });

    if ticket_v2_exe.is_file() {
        let _ = std::fs::copy(&ticket_v2_exe, &ticket_v2_bundle);
    } else {
        let _ = std::fs::write(&ticket_v2_bundle, []);
        println!("cargo:warning=ChamadosServicosTI-v2.exe ausente; execute desktop\\chamados-servicos-ti-v2\\Build-Pacote-Homologacao.ps1 antes dos novos setups");
    }

    if ticket_icon.is_file() {
        let _ = std::fs::copy(&ticket_icon, &ticket_icon_bundle);
    } else {
        let _ = std::fs::write(&ticket_icon_bundle, []);
        println!("cargo:warning=Ícone do Chamados Serviços TI ausente; será usado o ícone do EXE");
    }

    emit_env(
        &pc_bundle,
        &servidor_bundle,
        &ticket_v2_bundle,
        &ticket_icon_bundle,
    );
}

fn emit_env(pc: &Path, servidor: &Path, ticket_v2: &Path, ticket_icon: &Path) {
    println!("cargo:rustc-env=BELARC_PC_BUNDLE_ZIP={}", pc.display());
    println!(
        "cargo:rustc-env=BELARC_SERVIDOR_BUNDLE_ZIP={}",
        servidor.display()
    );
    println!(
        "cargo:rustc-env=BELARC_TICKET_V2_BUNDLE={}",
        ticket_v2.display()
    );
    println!(
        "cargo:rustc-env=BELARC_TICKET_ICON_BUNDLE={}",
        ticket_icon.display()
    );
}
