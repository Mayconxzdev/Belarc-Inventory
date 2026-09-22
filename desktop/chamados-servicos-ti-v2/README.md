# Chamados Serviços TI v2 — homologação isolada

Novo cliente desktop Windows para substituir futuramente o executável legado
`ChamadosServicosTI.exe`, sem alterar o binário atualmente distribuído.

## O que ele preserva e acrescenta

- Mantém o fluxo visual de menu lateral, **Novo chamado** e **Meus chamados**.
- Identifica o PC pelo `C:\ProgramData\BelarcInventory\config.toml` já criado pelo agente.
- Não solicita usuário ou senha ao colaborador e não entrega o token do agente ao frontend.
- Carrega destinos de chamados dinamicamente do servidor.
- Só mostra **Chamados recebidos** se o PC tiver setores configurados em `Por PC > Chamados`.
- Mostra notificação Windows enquanto o aplicativo está aberto e conectado.

## Limites desta primeira versão de homologação

O aplicativo não substitui automaticamente o cliente em produção nem instala
inicialização automática sozinho. A atualização segura é feita em duas fases:

1. **Pilot** — instala a v2 ao lado da versão anterior e cria o atalho
   `Chamados Serviços (novo)`.
2. **Promote** — somente após validação, troca o atalho principal e preserva o
   antigo como `Chamados Serviços (legado)` para retorno imediato.

O pacote não altera `belarc-agent.exe`, serviço, tarefas do agendador, coleta,
inventário ou `C:\ProgramData\BelarcInventory\config.toml`.

## Desenvolvimento e build

```powershell
cd desktop\chamados-servicos-ti-v2
go mod tidy
go build -tags "desktop,wv2runtime.download,production" -ldflags "-w -s -H windowsgui" -o build\ChamadosServicosTI-v2.exe .
```

Para gerar uma pasta autocontida de homologação (executável, hash, manifesto,
instalação e rollback):

```powershell
.\Build-Pacote-Homologacao.ps1
```

Veja as instruções completas de piloto, promoção e retorno em
[`deploy/README.md`](deploy/README.md).

Para executar em homologação, o computador precisa apontar o arquivo de
configuração do agente para o servidor isolado, nunca para o servidor de produção.
