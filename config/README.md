# Perfil da empresa

Arquivo central de personalização para implantação em produção ou demonstração no portfólio.

## Arquivo principal

| Arquivo | Uso |
|---------|-----|
| `config/company-profile.json` | Perfil de exemplo versionado, sem dados reais |
| `config/company-profile.json` | Ativo no build / demo portfólio (GitHub) |

Para deploy na LAN: edite `.production.json` e rode `.\deploy-producao.ps1`.

## Campos

| Campo | Descrição |
|-------|-----------|
| `company_name` | Nome exibido na documentação |
| `erp_name` | Nome do ERP corporativo |
| `erp_server_ip` | IP do servidor ERP na LAN (ping nos coletores) |
| `inventory_server_ip` | IP sugerido do servidor de inventário |
| `erp_branches[]` | Filiais/clientes ERP (caminho de instalação, temp) |
| `banking_app` | App bancário monitorado (`id` + `label`) |
| `ui.*` | Rótulos do dashboard (campos ERP, mensagens) |

## Uso

1. Copie e edite para seu ambiente:

```powershell
Copy-Item config\company-profile.json config\company-profile.local.json
# Edite IPs, nomes de ERP e filiais
$env:BELARC_COMPANY_PROFILE = (Resolve-Path config\company-profile.local.json)
```

2. Sincronize coletores após alterar o JSON:

```powershell
.\sync-collectors.ps1
```

3. Reinicie o servidor para carregar o perfil.

## Onde o perfil é lido

| Componente | Caminho |
|------------|---------|
| Servidor Rust | `BELARC_COMPANY_PROFILE` ou `config/company-profile.json` |
| Coletores PS1 | `agent/collectors/_company-profile.json` (copiado pelo sync) |
| Dashboard | `GET /api/company-profile` |
| Manutenção | `manutencao-pc.ps1` via `_load-profile.ps1` |

## Screenshots do portfólio

Ao adicionar prints de produção, **substitua** os placeholders em `docs/assets/screenshots/` e atualize o README. Oculte dados sensíveis mesmo em ambiente interno.
