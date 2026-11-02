<#
.SYNOPSIS
Run one real shell command through the deployed console, on the console host.

.DESCRIPTION
The browser gate proves the page's shape against an echo. This proves the other half: the console's
operator session, a real device channel, a real pty, and a marker that comes back from the shell.

It reads the operator token from its file on this host and never prints it, the session token or the
command output. Run it AFTER a console deploy; a marker failure is a real deployment failure.

.PARAMETER BaseUrl
The console's loopback address. Default http://127.0.0.1:19000.

.PARAMETER OperatorTokenFile
The file C:\SummriseConsole\run-console.cmd passes to --operator-token-file.

.PARAMETER Marker
The prefix of the marker; a short guid is appended per run.
#>
param(
  [string]$BaseUrl = 'http://127.0.0.1:19000',
  [string]$OperatorTokenFile = 'C:\SummriseConsole\operator.token',
  [string]$Marker = 'summrise-real-check'
)

$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath $OperatorTokenFile)) {
  throw "operator token file not found: $OperatorTokenFile"
}

$operator = (Get-Content -LiteralPath $OperatorTokenFile -Raw).Trim()
$auth = Invoke-RestMethod -Method Post -Uri "$BaseUrl/auth" -ContentType 'application/json' -Body (@{ token = $operator } | ConvertTo-Json)
$headers = @{ Authorization = "Bearer $($auth.token)" }

$fleet = Invoke-RestMethod -Uri "$BaseUrl/fleet" -Headers $headers
$device = $fleet | Where-Object { $_.online } | Select-Object -First 1
if (-not $device) { throw 'no online device in /fleet' }
Write-Output ("device=" + $device.name + " id8=" + $device.id.Substring(0, 8) + " online=" + $device.online)

function Invoke-Mcp([string]$name, [hashtable]$arguments) {
  $payload = @{
    device_id = $device.id
    request = @{
      jsonrpc = '2.0'
      id = 1
      method = 'tools/call'
      params = @{ name = $name; arguments = $arguments }
    }
  } | ConvertTo-Json -Depth 12
  $reply = Invoke-RestMethod -Method Post -Uri "$BaseUrl/mcp" -Headers $headers -ContentType 'application/json' -Body $payload
  if ($reply.result.isError) { throw ("tool error: " + $reply.result.content[0].text) }
  return $reply.result.content[0].text
}

$session = $null
try {
  $session = (Invoke-Mcp 'terminal.create' @{ kind = 'pty' } | ConvertFrom-Json).id
  Write-Output ("terminal_created=" + $session)

  $unique = $Marker + '-' + [guid]::NewGuid().ToString('N').Substring(0, 8)
  Invoke-Mcp 'terminal.write' @{ id = $session; data = ("echo " + $unique + "`r") } | Out-Null
  Start-Sleep -Milliseconds 1200
  $read = Invoke-Mcp 'terminal.read' @{ id = $session; cursor = 0 } | ConvertFrom-Json

  if ($read.data -notmatch [regex]::Escape($unique)) {
    throw "the marker did not come back from terminal.read (bytes=" + $read.data.Length + ")"
  }
  Write-Output ("marker_seen=True bytes=" + $read.data.Length + " cursor=" + $read.cursor)
  Write-Output 'REAL_TERMINAL_OK'
}
finally {
  if ($session) {
    try { Invoke-Mcp 'terminal.close' @{ id = $session } | Out-Null } catch { }
  }
}
