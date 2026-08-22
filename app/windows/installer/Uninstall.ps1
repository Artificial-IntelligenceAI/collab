# Removes collab. Leaves ~/.collab-* alone: those are your channel keys, your
# messages and your settings, and an uninstaller that deletes the keys deletes
# every conversation on the other machine's terms too. Delete them by hand if
# you mean to.
$ErrorActionPreference = 'SilentlyContinue'
Get-Process Collab, collab | Stop-Process -Force
Start-Sleep -Milliseconds 700
$dest = Join-Path $env:LOCALAPPDATA 'Programs\Collab'
foreach ($dir in @(
    (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'),
    [Environment]::GetFolderPath('Desktop'))) {
  Remove-Item (Join-Path $dir 'collab.lnk') -Force
}
Remove-Item 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Collab' -Recurse -Force
Remove-Item $dest -Recurse -Force
Write-Host '  collab removed. Your keys and messages in %USERPROFILE%\.collab-* were left alone.'
