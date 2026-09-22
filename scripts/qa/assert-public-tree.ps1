[CmdletBinding()]
param([string]$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path)

$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path -LiteralPath $Root).Path
$blockedPaths = @('bk','release','output','tmp','_recovered','data','target')
$blockedPatterns = @(
  '(?i)\\\\nas-novo\\',
  '(?i)192\.168\.',
  '(?i)BEGIN [A-Z ]*PRIVATE KEY',
  '(?i)BELARC_TI_PASSWORD=',
  '(?i)agent_token\s*=\s*"[^"\s]{16,}'
)
$extensions = @('.rs','.ps1','.js','.html','.css','.json','.md','.toml','.yml','.yaml')
$errors = [Collections.Generic.List[string]]::new()

$files = @(git -C $Root ls-files)
foreach ($path in $blockedPaths) {
  if ($files | Where-Object { $_ -eq $path -or $_ -like "$path/*" }) { $errors.Add("diretório versionado proibido: $path") }
}
foreach ($relative in $files) {
  if ($relative -eq 'scripts/qa/assert-public-tree.ps1') { continue }
  $file = Join-Path $Root $relative
  if (-not (Test-Path -LiteralPath $file) -or $extensions -notcontains [IO.Path]::GetExtension($file).ToLowerInvariant()) { continue }
  $content = [IO.File]::ReadAllText($file)
  foreach ($pattern in $blockedPatterns) {
    if ($content -match $pattern) { $errors.Add("conteúdo potencialmente sensível: $relative [$pattern]") }
  }
}

if ($errors.Count) {
  $errors | ForEach-Object { Write-Error $_ }
  throw "A árvore pública falhou na validação de sanitização ($($errors.Count) ocorrência(s))."
}
Write-Host '[PASS] árvore pública sem dados corporativos ou artefatos bloqueados' -ForegroundColor Green
