# collab — installer.
#
# Deliberately a readable script rather than a compiled setup program. A
# compiled one would have meant shipping a second copy of the .NET runtime,
# roughly 130 MB, to do work that amounts to copying a folder and making two
# shortcuts. This is also inspectable by whoever runs it, which a setup.exe
# handed over the internet is not.
#
# Installs per-user, so it never asks for an administrator.
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$dest = Join-Path $env:LOCALAPPDATA 'Programs\Collab'

Write-Host ''
Write-Host '  collab' -ForegroundColor Cyan
Write-Host '  messages between two machines while you work on the same thing'
Write-Host ''

if (-not (Test-Path (Join-Path $here 'Collab.exe'))) {
  Write-Host '  Collab.exe is not next to this script.' -ForegroundColor Red
  Write-Host '  Extract the whole zip first, then run Install.cmd from inside it.'
  Read-Host '  Press Enter to close'
  exit 1
}

# A running copy holds its own file open, so stop it before replacing anything.
Get-Process Collab, collab -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 700

Write-Host "  installing to $dest"
New-Item -ItemType Directory -Force -Path $dest, (Join-Path $dest 'bin') | Out-Null
Copy-Item (Join-Path $here 'Collab.exe')     $dest -Force
Copy-Item (Join-Path $here 'bin\collab.exe') (Join-Path $dest 'bin') -Force
if (Test-Path (Join-Path $here 'collab.png')) { Copy-Item (Join-Path $here 'collab.png') $dest -Force }
Copy-Item (Join-Path $here 'Uninstall.ps1') $dest -Force

# Shortcuts. The app makes its own Start Menu entry on first run because
# Windows will not attribute a toast to an unregistered program, but making it
# here too means it is there before the app has ever been opened.
$shell = New-Object -ComObject WScript.Shell
foreach ($dir in @(
    (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'),
    [Environment]::GetFolderPath('Desktop'))) {
  $lnk = $shell.CreateShortcut((Join-Path $dir 'collab.lnk'))
  $lnk.TargetPath = Join-Path $dest 'Collab.exe'
  $lnk.WorkingDirectory = $dest
  $lnk.Description = 'collab - messages from the other machine'
  $lnk.Save()
}
Write-Host '  shortcuts added to the Start Menu and the Desktop'

# So it appears in Installed Apps like anything else, and can be removed there.
$key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Collab'
New-Item -Path $key -Force | Out-Null
Set-ItemProperty $key DisplayName     'collab'
Set-ItemProperty $key DisplayIcon     (Join-Path $dest 'Collab.exe')
Set-ItemProperty $key InstallLocation $dest
Set-ItemProperty $key Publisher       'Tankun Sriket'
Set-ItemProperty $key NoModify        1 -Type DWord
Set-ItemProperty $key NoRepair        1 -Type DWord
Set-ItemProperty $key UninstallString ('powershell -ExecutionPolicy Bypass -File "' + (Join-Path $dest 'Uninstall.ps1') + '"')

# Register the MCP server so this person's Claude can read and post here.
# Deliberately in ONE config file only. The same server registered under the
# same name in both Claude Code's config and the desktop app's caused four
# hours of phantom refusals on the other machine: the desktop app's copy
# shadowed Claude Code's, tool calls landed on a server with no session id,
# and every post was refused with a message about not having joined.
$cliCfg  = Join-Path $env:USERPROFILE '.claude.json'
$deskCfg = Join-Path $env:APPDATA 'Claude\claude_desktop_config.json'
$target = $null
$other = $null
$which = ''
if (Test-Path $cliCfg) {
  $target = $cliCfg; $other = $deskCfg; $which = 'Claude Code'
} elseif (Test-Path $deskCfg) {
  $target = $deskCfg; $other = $cliCfg; $which = 'the Claude desktop app'
}

$exe = Join-Path $dest 'bin\collab.exe'
if ($target) {
  try {
    Copy-Item $target ($target + '.backup-' + (Get-Date -Format 'yyyyMMdd-HHmmss')) -Force
    $cfg = Get-Content $target -Raw | ConvertFrom-Json
    if ($null -eq $cfg.mcpServers) {
      $cfg | Add-Member -NotePropertyName mcpServers -NotePropertyValue ([pscustomobject]@{}) -Force
    }
    $entry = [pscustomobject]@{ command = $exe; args = @('mcp') }
    $cfg.mcpServers | Add-Member -NotePropertyName collab -NotePropertyValue $entry -Force
    $cfg | ConvertTo-Json -Depth 32 | Set-Content $target -Encoding UTF8
    Write-Host "  registered with $which - restart it to pick up the tools"
    if (Test-Path $other) {
      Write-Host '  (the other Claude config was left alone on purpose: the same'
      Write-Host '   server registered in both shadows one and breaks posting)'
    }
  } catch {
    Write-Host "  could not register with $which - add this to its mcpServers yourself:" -ForegroundColor Yellow
    Write-Host ('    "collab": { "command": "' + ($exe -replace '\\', '\\\\') + '", "args": ["mcp"] }')
  }
} else {
  Write-Host '  no Claude config found, so nothing was registered.'
  Write-Host '  If you use Claude, add this to its mcpServers:'
  Write-Host ('    "collab": { "command": "' + ($exe -replace '\\', '\\\\') + '", "args": ["mcp"] }')
}

Write-Host ''
Write-Host '  done.' -ForegroundColor Green
Write-Host '  Next: open collab, press the # button, and paste the invite the other person sent you.'
Write-Host ''
Start-Process (Join-Path $dest 'Collab.exe')
Start-Sleep -Seconds 2
