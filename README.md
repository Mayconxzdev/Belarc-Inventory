# Belarc Inventory

[![CI](https://github.com/Mayconxzdev/Belarc-Inventory/actions/workflows/ci.yml/badge.svg)](https://github.com/Mayconxzdev/Belarc-Inventory/actions/workflows/ci.yml)
![Windows](https://img.shields.io/badge/plataforma-Windows-0078D4)
![Rust](https://img.shields.io/badge/backend-Rust-orange)
![License](https://img.shields.io/badge/licen%C3%A7a-MIT-green)

Sistema interno para inventário de PCs Windows, saúde operacional, auditoria e chamados por setor em redes LAN/VPN privadas.

Ele foi construído por **Maycon Ferreira** e **Diogo Rodrigues** a partir de uma necessidade operacional: ter contexto técnico confiável sobre cada estação sem depender de planilhas, memória da equipe ou conferência manual máquina por máquina.

> O nome é inspirado em ferramentas de inventário de TI. Este projeto não é afiliado à Belarc Inc.

## O que demonstra

- Agente Windows leve em Rust com coletores PowerShell/CIM para hardware, software, rede, certificados, eventos e segurança.
- Servidor Axum + SQLite com inventário centralizado, alertas, conformidade, histórico e relatórios Markdown.
- Dashboard para TI com visão de frota, gestão por computador, diretório e Kanban de chamados.
- Portal de chamados por computador, com setores, conversa, anexos e recebimento direcionado.
- Instalação separada para servidor, agente, estação TI e usuário final.

## Ver a demo local

Requisitos: Windows, PowerShell 5.1+ e Rust estável.

```powershell
git clone https://github.com/Mayconxzdev/Belarc-Inventory.git
cd Belarc-Inventory
cargo build -p belarc-server -p belarc-agent
.\scripts\qa\isolated-smoke-test.ps1 -Build
```

O smoke test usa perfil, banco, tickets e compartilhamento simulados dentro de `test-data\isolated`. Ele não instala serviço Windows, não cria tarefa agendada e não acessa rede corporativa.

## Compilar

```powershell
cargo build --workspace
cargo test --workspace
```

Os executáveis e instaladores não são versionados nem distribuídos como release nesta primeira edição. A compilação local é o caminho suportado até existir uma homologação física de instalação/upgrade e assinatura de código.

## Implantar em LAN/VPN privada

1. Ajuste `config/company-profile.json` para sua organização e mantenha dados reais fora do Git.
2. Instale e inicie `BelarcServidor` no computador de TI com URL LAN/VPN explícita.
3. Crie a conta inicial de TI e limite o firewall à subnet autorizada.
4. Gere uma matrícula temporária no painel de TI.
5. Instale `BelarcPC` nas estações usando URL e matrícula emitidas pela TI.

Leia o [guia de implantação LAN/VPN](docs/deployment-lan.md) antes de instalar em uma rede real.

## Arquitetura

```text
PC Windows
  agente Rust + coletores PowerShell
            │ token do agente / HTTP interno
            ▼
Servidor LAN/VPN
  Axum + SQLite + dashboard + portal de chamados
            │
            ├── visão da TI: inventário, alertas, conformidade e Kanban
            └── visão do usuário: chamados do computador e setores recebidos
```

## Segurança e limites

- Uso restrito a LAN/VPN privada; não exponha este servidor diretamente na internet.
- Senhas de ERP, compartilhamentos e aplicações não são armazenadas por esta edição pública.
- Tokens, bancos, anexos, relatórios, certificados, perfis locais e logs não pertencem ao Git.
- Execute `scripts\qa\assert-public-tree.ps1` antes de publicar uma alteração.

Veja todos os limites em [SECURITY.md](SECURITY.md).

## Documentação

- [Catálogo de coletores](docs/collectors-catalog.md)
- [Implantação LAN/VPN](docs/deployment-lan.md)
- [Dados de demonstração](docs/demo/inventory-sample.json)

## Licença

MIT. Consulte [LICENSE](LICENSE).
