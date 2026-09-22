# Implantação em LAN/VPN privada

Belarc Inventory foi projetado para redes privadas Windows. Não exponha o servidor diretamente na internet.

## Primeira configuração

1. Crie um perfil próprio a partir de `config/company-profile.json`; mantenha o arquivo local fora do Git.
2. Defina um endereço fixo ou DNS interno para o servidor, por exemplo `http://inventario.intra:8080`.
3. Instale o servidor em um PC Windows administrado, com acesso apenas à subnet/VPN necessária.
4. Crie a conta inicial da TI e guarde a senha em um cofre. O produto armazena apenas o hash da senha de TI.
5. Restrinja o firewall à subnet interna. A porta padrão recomendada para esta edição é `8080`.

## Matrícula de um agente

Um PC só deve ser matriculado a partir de uma sessão TI. Gere no painel um token de matrícula de uso único, válido por 24 horas, e informe a URL LAN/VPN e o token ao instalador do agente. Depois do primeiro registro, o token fica associado somente àquela máquina e não pode matricular outro PC.

Não compartilhe tokens por e-mail, tickets públicos ou capturas de tela. Revogue a matrícula se o PC for descartado ou transferido.

## Dados sensíveis

Esta edição não armazena senhas de ERP, NAS ou aplicações. Use um cofre de senhas corporativo e registre no Belarc apenas a referência operacional necessária.

Instalações legadas podem ter colunas históricas de senha no SQLite. Faça backup e execute uma migração manual planejada; esta edição não apaga dados antigos automaticamente.

## Limites

- Windows 10/11 e PowerShell 5.1+ nos agentes.
- LAN/VPN privada; TLS, proxy reverso e identidade externa não fazem parte desta primeira distribuição.
- O NAS é opcional e não é pré-requisito para inventário, dashboard ou chamados locais.
