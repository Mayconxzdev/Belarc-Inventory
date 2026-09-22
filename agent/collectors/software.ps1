$ErrorActionPreference = 'SilentlyContinue'

. (Join-Path $PSScriptRoot '_load-profile.ps1')
. (Join-Path $PSScriptRoot '_user-context.ps1')
$companyProfile = Get-CompanyProfile -BaseDir $PSScriptRoot
$bankingApp = $companyProfile.banking_app

$uninstallPaths = @(
    'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*',
    'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*'
) + @(Get-BelarcUserRegistryUninstallPaths)

$programs = @()
foreach ($path in $uninstallPaths) {
    Get-ItemProperty $path -ErrorAction SilentlyContinue | ForEach-Object {
        if ($_.DisplayName) {
            $programs += [ordered]@{
                display_name    = $_.DisplayName
                version         = $_.DisplayVersion
                publisher       = $_.Publisher
                install_date    = $_.InstallDate
                install_location = $_.InstallLocation
            }
        }
    }
}

$programs = $programs | Sort-Object display_name -Unique { $_.display_name }

$services = @(Get-CimInstance Win32_Service | Where-Object { $_.StartMode -ne 'Disabled' } | Select-Object -First 100 | ForEach-Object {
    [ordered]@{ name = $_.Name; display_name = $_.DisplayName; state = $_.State; start_mode = $_.StartMode; path = $_.PathName }
})

$startup = @()
$runKeys = @(
    'HKLM:\Software\Microsoft\Windows\CurrentVersion\Run'
) + @(Get-BelarcUserRunKeys)
foreach ($key in $runKeys) {
    if (Test-Path $key) {
        Get-ItemProperty $key | Get-Member -MemberType NoteProperty | Where-Object { $_.Name -notin @('PSPath','PSParentPath','PSChildName','PSDrive','PSProvider') } | ForEach-Object {
            $startup += [ordered]@{ name = $_.Name; command = (Get-ItemProperty $key).($_.Name); source = $key }
        }
    }
}

$browsers = @()
@(
    @{ name = 'Google Chrome'; path = "${env:ProgramFiles}\Google\Chrome\Application\chrome.exe" },
    @{ name = 'Microsoft Edge'; path = "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe" },
    @{ name = 'Mozilla Firefox'; path = "${env:ProgramFiles}\Mozilla Firefox\firefox.exe" }
) | ForEach-Object {
    if (Test-Path $_.path) {
        $ver = (Get-Item $_.path).VersionInfo.FileVersion
        $browsers += [ordered]@{ name = $_.name; version = $ver }
    }
}

function Get-AppPathExe($name) {
    foreach ($root in @('HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths', 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\App Paths')) {
        $key = Join-Path $root $name
        if (Test-Path $key) {
            $p = (Get-ItemProperty $key -ErrorAction SilentlyContinue).'(default)'
            if ($p -and (Test-Path $p)) { return $p }
        }
    }
    return $null
}

function Find-StandardExe($candidates) {
    foreach ($p in $candidates) { if (Test-Path $p) { return $p } }
    return $null
}

function Get-ExeFileVersion($path) {
    try {
        $vi = (Get-Item $path -ErrorAction Stop).VersionInfo
        if ($vi.ProductVersion) { return $vi.ProductVersion }
        return $vi.FileVersion
    } catch { return $null }
}

function Detect-Affinity($programs) {
    $result = [ordered]@{
        installed = $false
        version   = $null
        detail    = $null
        product   = 'Affinity Canva'
    }

    $exeCandidates = @(
        "${env:ProgramFiles}\Affinity\Affinity\Affinity.exe",
        "${env:ProgramFiles(x86)}\Affinity\Affinity\Affinity.exe",
        "${env:ProgramFiles}\Affinity\Designer 2\AffinityDesigner.exe",
        "${env:ProgramFiles}\Affinity\Photo 2\AffinityPhoto.exe",
        "${env:ProgramFiles}\Affinity\Publisher 2\AffinityPublisher.exe"
    )
    foreach ($p in $exeCandidates) {
        if (Test-Path $p) {
            $result.installed = $true
            $result.detail = $p
            $result.version = Get-ExeFileVersion $p
            return $result
        }
    }

    $appPath = Get-AppPathExe 'Affinity.exe'
    if ($appPath) {
        $result.installed = $true
        $result.detail = $appPath
        $result.version = Get-ExeFileVersion $appPath
        return $result
    }

    try {
        $appx = Get-AppxPackage -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -match 'Canva\.Affinity|^Affinity$|Serif' } |
            Sort-Object Version -Descending |
            Select-Object -First 1
        if ($appx) {
            $result.installed = $true
            $result.version = $appx.Version.ToString()
            $result.detail = $appx.InstallLocation
            return $result
        }
    } catch {}

    $winApps = "${env:ProgramFiles}\WindowsApps"
    if (Test-Path $winApps) {
        $pkg = Get-ChildItem $winApps -Directory -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -match '^Canva\.Affinity_' } |
            Sort-Object Name -Descending |
            Select-Object -First 1
        if ($pkg) {
            $exe = Join-Path $pkg.FullName 'App\Affinity.exe'
            if (Test-Path $exe) {
                $result.installed = $true
                $result.detail = $exe
                $result.version = Get-ExeFileVersion $exe
                if (-not $result.version -and $pkg.Name -match '_(\d+\.\d+\.\d+\.\d+)_') {
                    $result.version = $Matches[1]
                }
                return $result
            }
        }
    }

    $prog = Test-ProgramMatch $programs @(
        'affinity canva', 'affinity v3', 'affinity 3', 'canva affinity',
        'affinity designer', 'affinity photo', 'affinity publisher', 'serif affinity', ' affinity '
    )
    if ($prog) {
        $result.installed = $true
        $result.version = $prog.version
        $result.detail = $prog.display_name
        return $result
    }

    $roots = @("${env:ProgramFiles}", "${env:ProgramFiles(x86)}", 'D:\', 'E:\')
    foreach ($root in $roots) {
        if (-not (Test-Path $root)) { continue }
        $hit = Get-ChildItem $root -Directory -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -match '^Affinity$|^Serif' } |
            ForEach-Object {
                Get-ChildItem $_.FullName -Recurse -Include 'Affinity.exe','AffinityDesigner.exe','AffinityPhoto.exe' -ErrorAction SilentlyContinue |
                    Select-Object -First 1
            } | Select-Object -First 1
        if ($hit) {
            $result.installed = $true
            $result.detail = $hit.FullName
            $result.version = Get-ExeFileVersion $hit.FullName
            return $result
        }
    }

    $result
}

function Test-ProgramMatch($programs, $patterns) {
    foreach ($prog in $programs) {
        $n = " $($prog.display_name) ".ToLower()
        foreach ($pat in $patterns) {
            if ($n -like "*$($pat.ToLower())*") { return $prog }
        }
    }
    return $null
}

function New-StdApp($id, $label, $category, $installed, $version, $detail) {
    [ordered]@{
        id        = $id
        label     = $label
        category  = $category
        installed = [bool]$installed
        version   = $version
        detail    = $detail
    }
}

# App Itau fica em %LOCALAPPDATA%\Aplicativo Itau\ — invisivel ao servico SYSTEM sem varrer perfis de usuario
function Find-BankingAppInstall {
    $exeNames = @('banking_appaplicativo.exe', 'central.exe', 'banking_appaplicativo')

    $localRoots = [System.Collections.Generic.List[string]]::new()
    if ($env:LOCALAPPDATA) { [void]$localRoots.Add($env:LOCALAPPDATA) }
    foreach ($profile in Get-BelarcUserProfiles) {
        if ($profile.appdata_local) { [void]$localRoots.Add($profile.appdata_local) }
    }
    if (Test-Path 'C:\Users') {
        Get-ChildItem 'C:\Users' -Directory -ErrorAction SilentlyContinue | ForEach-Object {
            $p = Join-Path $_.FullName 'AppData\Local'
            if (Test-Path $p) { [void]$localRoots.Add($p) }
        }
    }

    $seen = @{}
    foreach ($local in $localRoots) {
        if (-not $local -or $seen[$local]) { continue }
        $seen[$local] = $true

        $folders = @()
        foreach ($name in @('Aplicativo Itau', 'Aplicativo Aplicação bancária', 'Itau')) {
            $p = Join-Path $local $name
            if (Test-Path $p) { $folders += $p }
        }
        if (-not $folders.Count) {
            $folders = @(Get-ChildItem $local -Directory -ErrorAction SilentlyContinue |
                Where-Object { $_.Name -match 'Aplicativo\s+Ita[uú]?|Itau' } |
                Select-Object -ExpandProperty FullName)
        }

        foreach ($base in $folders) {
            foreach ($exeName in $exeNames) {
                $candidate = Join-Path $base $exeName
                if (Test-Path $candidate) {
                    $ver = $null
                    try { $ver = (Get-Item $candidate).VersionInfo.ProductVersion } catch {}
                    $user = ($local -replace '\\AppData\\Local$', '')
                    $userLabel = Split-Path $user -Leaf
                    return @{
                        installed = $true
                        detail    = $candidate
                        version   = $ver
                        user      = $userLabel
                    }
                }
            }
            $exe = Get-ChildItem $base -Filter 'banking_appaplicativo.exe' -Recurse -Depth 3 -ErrorAction SilentlyContinue |
                Select-Object -First 1
            if ($exe) {
                $ver = $null
                try { $ver = $exe.VersionInfo.ProductVersion } catch {}
                $user = ($local -replace '\\AppData\\Local$', '')
                return @{
                    installed = $true
                    detail    = $exe.FullName
                    version   = $ver
                    user      = (Split-Path $user -Leaf)
                }
            }
        }
    }

    foreach ($root in @("${env:ProgramFiles}\Itau", "${env:ProgramFiles(x86)}\Itau")) {
        if (Test-Path $root) {
            return @{ installed = $true; detail = $root; version = $null; user = $null }
        }
    }

    foreach ($menu in @("$env:ProgramData\Microsoft\Windows\Start Menu\Programs", "$env:APPDATA\Microsoft\Windows\Start Menu\Programs")) {
        if (-not (Test-Path $menu)) { continue }
        foreach ($profile in Get-BelarcUserProfiles) {
            $userMenu = Join-Path $profile.appdata 'Microsoft\Windows\Start Menu\Programs'
            if (Test-Path $userMenu) {
                $lnk = Get-ChildItem $userMenu -Recurse -Filter '*.lnk' -ErrorAction SilentlyContinue |
                    Where-Object { $_.Name -match 'Itau|Aplicação bancária|Aplicativo Itau' } | Select-Object -First 1
                if ($lnk) {
                    return @{ installed = $true; detail = $lnk.FullName; version = $null; user = $profile.user }
                }
            }
        }
        $lnk = Get-ChildItem $menu -Recurse -Filter '*.lnk' -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -match 'Itau|Aplicação bancária|Aplicativo Itau' } | Select-Object -First 1
        if ($lnk) {
            return @{ installed = $true; detail = $lnk.FullName; version = $null; user = $null }
        }
    }

    @{ installed = $false; detail = $null; version = $null; user = $null }
}

$officeSuite = $programs | Where-Object {
    $_.display_name -match 'Microsoft Office (Professional|Home and Business|Standard|LTSC|365)|Microsoft 365'
} | Select-Object -First 1

$excelPath = Get-AppPathExe 'excel.exe'
if (-not $excelPath) {
    $excelPath = Find-StandardExe @(
        'C:\Program Files\Microsoft Office\root\Office16\EXCEL.EXE',
        'C:\Program Files (x86)\Microsoft Office\root\Office16\EXCEL.EXE',
        'C:\Program Files\Microsoft Office\Office16\EXCEL.EXE'
    )
}
$excelProg = Test-ProgramMatch $programs @('microsoft excel')
$excelInstalled = [bool]($excelPath -or $excelProg -or ($officeSuite -and $officeSuite.display_name -match 'Professional|Home and Business|Standard|365|LTSC'))

$wordPath = Get-AppPathExe 'winword.exe'
if (-not $wordPath) {
    $wordPath = Find-StandardExe @(
        'C:\Program Files\Microsoft Office\root\Office16\WINWORD.EXE',
        'C:\Program Files (x86)\Microsoft Office\root\Office16\WINWORD.EXE',
        'C:\Program Files\Microsoft Office\Office16\WINWORD.EXE'
    )
}
$wordProg = Test-ProgramMatch $programs @('microsoft word')
$wordInstalled = [bool]($wordPath -or $wordProg -or ($officeSuite -and $officeSuite.display_name -match 'Professional|Home and Business|Standard|365|LTSC'))

$affinityInfo = Detect-Affinity $programs

$erpBranchDefs = @()
foreach ($branch in @($companyProfile.erp_branches)) {
    $erpBranchDefs += @{ id = $branch.id; label = $branch.label; path = $branch.install_path; company = $branch.label }
}

$excelVer = if ($excelProg) { $excelProg.version } elseif ($officeSuite) { $officeSuite.version } else { $null }
$excelDet = if ($excelPath) { $excelPath } elseif ($officeSuite) { "Via $($officeSuite.display_name)" } else { $null }
$wordVer = if ($wordProg) { $wordProg.version } elseif ($officeSuite) { $officeSuite.version } else { $null }
$wordDet = if ($wordPath) { $wordPath } elseif ($officeSuite) { "Via $($officeSuite.display_name)" } else { $null }
$officeVer = if ($officeSuite) { $officeSuite.version } else { $null }
$officeDet = if ($officeSuite) { $officeSuite.display_name } else { $null }

$standardApps = @(
    (New-StdApp 'excel' 'Microsoft Excel' 'escritorio' $excelInstalled $excelVer $excelDet),
    (New-StdApp 'word' 'Microsoft Word' 'escritorio' $wordInstalled $wordVer $wordDet),
    (New-StdApp 'office' 'Microsoft Office' 'escritorio' [bool]$officeSuite $officeVer $officeDet)
)

foreach ($cy in $erpBranchDefs) {
    $exists = Test-Path $cy.path
    $ver = $null
    if ($exists) { try { $ver = (Get-Item $cy.path).VersionInfo.ProductVersion } catch {} }
    $standardApps += New-StdApp $cy.id $cy.label 'erp' $exists $ver $cy.path
}

$standardApps += New-StdApp 'affinity' 'Affinity Canva' 'design' $affinityInfo.installed $affinityInfo.version $affinityInfo.detail

$namedApps = @(
    @{ id = 'autocad'; label = 'AutoCAD'; cat = 'engenharia'; pats = @('autocad') },
    @{ id = 'fusion'; label = 'Fusion 360'; cat = 'engenharia'; pats = @('fusion') },
    @{ id = 'libredraw'; label = 'LibreOffice Draw'; cat = 'escritorio'; pats = @('libreoffice') },
    @{ id = 'lightshot'; label = 'Lightshot'; cat = 'utilitario'; pats = @('lightshot') },
    @{ id = 'pdf24'; label = 'PDF24'; cat = 'utilitario'; pats = @('pdf24') },
    @{ id = 'sumatra'; label = 'Sumatra PDF'; cat = 'utilitario'; pats = @('sumatra') },
    @{ id = 'eset'; label = 'ESET'; cat = 'seguranca'; pats = @('eset endpoint', 'eset security', 'eset antivirus') },
    @{ id = 'gimp'; label = 'GIMP'; cat = 'design'; pats = @('gimp') },
    @{ id = 'vscode'; label = 'VS Code'; cat = 'dev'; pats = @('visual studio code', 'vscode') },
    @{ id = $bankingApp.id; label = $bankingApp.label; cat = 'bancario'; pats = @('banking_app', 'aplicativo banking_app', 'banking_appbank', 'gerenciador banking_app', 'banking_app empresas', 'banking_app desktop', 'banco', 'bank', 'aplicativo banc') }
)
foreach ($def in $namedApps) {
    $hit = Test-ProgramMatch $programs $def.pats
    $hitVer = if ($hit) { $hit.version } else { $null }
    $hitDet = if ($hit) { $hit.display_name } else { $null }
    $installed = [bool]$hit

    if ($def.id -eq $bankingApp.id) {
        if (-not $installed) {
            $bank = Find-BankingAppInstall
            if ($bank.installed) {
                $installed = $true
                $hitDet = $bank.detail
                $hitVer = $bank.version
                if ($bank.user) { $hitDet = "$($bank.detail) (usuario: $($bank.user))" }
            }
        }
    }

    $standardApps += New-StdApp $def.id $def.label $def.cat $installed $hitVer $hitDet
}

$result = [ordered]@{
    programs       = @($programs | Select-Object -First 500)
    program_count  = $programs.Count
    services       = $services
    startup        = $startup
    browsers       = $browsers
    office_suite   = $officeSuite
    standard_apps  = @($standardApps)
}

$result | ConvertTo-Json -Compress -Depth 6
