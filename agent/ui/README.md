# UI Tray (Tauri 2) — Opcional

O agente funciona como **Windows Service** sem interface gráfica.

Para adicionar tray icon com Tauri 2 no futuro:

```bash
cd agent/ui
npm create tauri-app@latest . -- --template vanilla-ts
```

A UI deve comunicar com o serviço via pipe nomeado ou arquivo de status em `%ProgramData%\BelarcInventory\status.json`.

Funcionalidades planejadas:
- Status da última coleta
- Forçar coleta T1/T2
- Ver erros recentes
- Link para relatório no servidor

Por ora, use o dashboard web em `http://<servidor>:8080`.
