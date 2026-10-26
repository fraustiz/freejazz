# Prépare une version publiable : compile en release (build.ps1), puis crée dans dist\
#   freejazz-v<version>-windows-x86_64.zip   (freejazz.exe, freejazz-cli.exe, README.md, LICENSE)
#   freejazz-v<version>-windows-x86_64.zip.sha256   (hash en minuscules, deux espaces, nom du fichier)
# La version est celle de Cargo.toml ([workspace.package]). À lancer depuis PowerShell.
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

$match = Select-String -Path Cargo.toml -Pattern '^\s*version\s*=\s*"(\d+\.\d+\.\d+)"' | Select-Object -First 1
if (-not $match) { throw "Version introuvable dans Cargo.toml." }
$version = $match.Matches[0].Groups[1].Value
$name = "freejazz-v$version-windows-x86_64"

& "$PSScriptRoot\build.ps1"
if ($LASTEXITCODE -ne 0) { throw "build.ps1 a échoué." }

$stage = Join-Path $env:TEMP "$name-stage"
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory $stage | Out-Null
foreach ($file in 'freejazz.exe', 'freejazz-cli.exe') { Copy-Item (Join-Path 'dist' $file) $stage }
foreach ($file in 'README.md', 'LICENSE') { Copy-Item $file $stage }

$zip = Join-Path 'dist' "$name.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip -CompressionLevel Optimal
Remove-Item $stage -Recurse -Force

$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLowerInvariant()
[System.IO.File]::WriteAllText((Join-Path (Resolve-Path 'dist') "$name.zip.sha256"), "$hash  $name.zip`n", (New-Object System.Text.UTF8Encoding($false)))

'{0}  {1:N1} Mo' -f "$name.zip", ((Get-Item $zip).Length / 1MB)
"$hash  $name.zip"
