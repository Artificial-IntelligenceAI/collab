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
# Take the MCP entry out of whichever config has it, so Claude does not keep
# trying to start a program that is gone.
foreach ($cfgPath in @((Join-Path $env:USERPROFILE '.claude.json'),
                       (Join-Path $env:APPDATA 'Claude\claude_desktop_config.json'))) {
  if (-not (Test-Path $cfgPath)) { continue }
  try {
    $cfg = Get-Content $cfgPath -Raw | ConvertFrom-Json
    if ($cfg.mcpServers.collab) {
      Copy-Item $cfgPath "$cfgPath.backup-$(Get-Date -Format yyyyMMdd-HHmmss)" -Force
      $cfg.mcpServers.PSObject.Properties.Remove('collab')
      $cfg | ConvertTo-Json -Depth 32 | Set-Content $cfgPath -Encoding UTF8
      Write-Host "  removed the collab entry from $cfgPath"
    }
  } catch { }
}
Write-Host '  collab removed. Your keys and messages in %USERPROFILE%\.collab-* were left alone.'
