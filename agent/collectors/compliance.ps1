$ErrorActionPreference = 'SilentlyContinue'

# Metadados para o servidor calcular o score (fonte única: compliance.rs).
# Não calcular health_score aqui — evita divergência com o dashboard.

$pendingUpdates = 0
try {
    $session = New-Object -ComObject Microsoft.Update.Session
    $searcher = $session.CreateUpdateSearcher()
    $result = $searcher.Search("IsInstalled=0")
    $pendingUpdates = $result.Updates.Count
} catch {}

$result = [ordered]@{
    pending_updates = $pendingUpdates
    evaluated_at    = (Get-Date).ToString('o')
}

$result | ConvertTo-Json -Compress -Depth 5
