# Compile Freejazz (appli) et freejazz-cli en release, puis copie les deux exécutables
# dans dist\. À lancer depuis PowerShell (le linker MSVC n'est pas vu par Git Bash).
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

cargo build --release -p freejazz -p freejazz-cli
if ($LASTEXITCODE -ne 0) { throw "La compilation a échoué (code $LASTEXITCODE)." }

New-Item -ItemType Directory -Force dist | Out-Null
foreach ($exe in 'freejazz.exe', 'freejazz-cli.exe') {
    Copy-Item (Join-Path 'target\release' $exe) (Join-Path 'dist' $exe) -Force
}
Get-ChildItem dist\*.exe | ForEach-Object {
    '{0}  {1:N1} Mo' -f $_.Name, ($_.Length / 1MB)
}
