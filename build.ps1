# Compile Freejazz (appli) et freejazz-cli en release, puis copie les deux exécutables
# dans dist\. À lancer depuis PowerShell (le linker MSVC n'est pas vu par Git Bash).
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

cargo build --release -p freejazz-winui -p freejazz -p freejazz-cli
if ($LASTEXITCODE -ne 0) { throw "La compilation a échoué (code $LASTEXITCODE)." }

New-Item -ItemType Directory -Force dist | Out-Null

# Runtime dependencies WinUI 3 (DLLs et PRIs)
Get-ChildItem -Path target\release\*.dll, target\release\*.pri | ForEach-Object {
    Copy-Item $_.FullName dist\ -Force
}

# freejazz-winui comme application principale freejazz.exe et freejazz-winui.exe
Copy-Item (Join-Path 'target\release' 'freejazz-winui.exe') (Join-Path 'dist' 'freejazz.exe') -Force
Copy-Item (Join-Path 'target\release' 'freejazz-winui.exe') (Join-Path 'dist' 'freejazz-winui.exe') -Force
Copy-Item (Join-Path 'target\release' 'freejazz.exe') (Join-Path 'dist' 'freejazz-egui.exe') -Force
Copy-Item (Join-Path 'target\release' 'freejazz-cli.exe') (Join-Path 'dist' 'freejazz-cli.exe') -Force

Get-ChildItem dist\*.exe | ForEach-Object {
    '{0}  {1:N1} Mo' -f $_.Name, ($_.Length / 1MB)
}
