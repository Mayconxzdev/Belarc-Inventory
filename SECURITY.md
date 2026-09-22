# Política de segurança

## Limite de confiança

Belarc Inventory é um produto para LAN/VPN privada. Não exponha a API ou o dashboard diretamente na internet.

- A conta da TI usa senha com hash bcrypt e sessão temporária.
- O painel administrativo e a emissão de tokens de agentes exigem sessão TI.
- O portal de chamados do computador usa uma sessão curta limitada ao próprio dispositivo e aos setores que ele recebe.
- Senhas de aplicações, compartilhamentos ou ERP não são armazenadas pela edição pública.

## Antes de publicar ou implantar

1. Execute `scripts\qa\assert-public-tree.ps1` antes de cada push.
2. Não versione bancos SQLite, tokens, relatórios, anexos, logs, certificados ou perfis locais.
3. Restrinja a porta do servidor à subnet/VPN autorizada.
4. Faça backup do diretório de dados antes de atualizar uma instalação existente.

## Limitações conhecidas

Esta primeira edição não inclui TLS próprio, assinatura de executáveis nem suporte para exposição pública. Utilize apenas em rede privada e siga o guia de [implantação LAN](docs/deployment-lan.md).

## Reporte responsável

Abra uma issue privada ou entre em contato pelos perfis dos mantenedores. Não anexe tokens, bancos ou dados de inventário ao reporte.
