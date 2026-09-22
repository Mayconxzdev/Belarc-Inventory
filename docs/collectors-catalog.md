# Catálogo de Coletores

| Coletor | Tier | Script | Dados principais |
|---------|------|--------|------------------|
| identity | T0/T1 | identity.ps1 | hostname, serial, UUID, uptime, usuário |
| os | T1 | os.ps1 | Windows version, activation, hotfixes, RDP |
| hardware | T1 | hardware.ps1 | CPU, RAM, placa-mãe, BIOS, chassis, monitores, teclado, mouse, discos, GPU, áudio, rede física, TPM |
| peripherals | T1 | peripherals.ps1 | Impressoras, USB, Bluetooth, câmeras, áudio PnP, entrada, gamepads |
| network | T1 | network.ps1 | IP LAN, NICs, SMB, drives, Wi-Fi SSIDs, firewall |
| remote_access | T1 | remote_access.ps1 | Tailscale (conectado, IP, startup), AnyDesk ID, TeamViewer |
| software | T1 | software.ps1 | Programas, serviços, startup, browsers |
| licensing | T2 | licensing.ps1 | Windows/Office/Adobe keys parciais |
| certificates | T2 | certificates.ps1 | Cert stores, expiração |
| security | T2 | security.ps1 | Defender, AV, BitLocker, admins |
| email | T1 | email.ps1 | Thunderbird perfis (%APPDATA%\\Thunderbird\\Profiles), e-mails, contas IMAP/SMTP, Outlook |
| peripherals | T2 | peripherals.ps1 | Impressoras, USB, scanners |
| logins | T1 | logins.ps1 | Usuários locais, grupos, sessões, histórico logon, credenciais salvas (sem senha) |
| event_logs | T1 | event_logs.ps1 | BSOD/tela azul, erros System/Application, falhas de serviço, minidumps |
| permissions | T2 | permissions.ps1 | Grupos locais, ACLs, GPO |
| compliance | T1 | compliance.ps1 | Score local, updates pendentes |
| runtime | T1 | runtime.ps1 | Docker, WSL, Hyper-V, Ollama |
| performance | T1 | performance.ps1 | CPU/RAM %, disco I/O, temperatura, top processos, Reliability Monitor, ping ERP (config), apps críticos |

Cada coletor retorna JSON via stdout. O agente grava em arquivo UTF-8 e calcula SHA-256 para delta sync.

**Fonte canônica:** `agent/collectors/`. Após `cargo build`, execute `.\sync-collectors.ps1` para copiar os scripts para `target/*/collectors/`.
