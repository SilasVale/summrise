<#
.SYNOPSIS
Prove the deployed console's hub routes against a real device.

.DESCRIPTION
Three facts the page depends on and no unit test can prove once deployed:
 1. `GET /fleet/:id/tools` returns the device's live catalogue, including `plugins.*` after the
    device carries the plugin host;
 2. a token can be minted whose scope names a plugin tool, and revoked;
 3. the plugin tool it names is the one the device advertises.

It reads the operator token from its file on this host and never prints it, the session token, the
created token or the tool output beyond counts and booleans.

.PARAMETER BaseUrl
The console's loopback address. Default http://127.0.0.1:19000.

.PARAMETER OperatorTokenFile
The file C:\SummriseConsole\run-console.cmd passes to --operator-token-file.

.PARAMETER PluginPath
The plugin directory on the device host. Default C:\SummrisePlugin\hello.
#>
param(
  [string]$BaseUrl = 'http://127.0.0.1:19000',
  [string]$OperatorTokenFile = 'C:\SummriseConsole\operator.token',
  [string]$PluginPath = 'C:\SummrisePlugin\hello'
)

$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath $OperatorTokenFile)) {
  throw "operator token file not found: $OperatorTokenFile"
}
if (-not (Test-Path -LiteralPath (Join-Path $PluginPath 'plugin.json'))) {
  throw "no plugin.json under $PluginPath; stage the example plugin there first"
}

$operator = (Get-Content -LiteralPath $OperatorTokenFile -Raw).Trim()
$auth = Invoke-RestMethod -Method Post -Uri "$BaseUrl/auth" -ContentType 'application/json' -Body (@{ token = $operator } | ConvertTo-Json)
$headers = @{ Authorization = "Bearer $($auth.token)" }

$fleet = Invoke-RestMethod -Uri "$BaseUrl/fleet" -Headers $headers
$device = $fleet | Where-Object { $_.online } | Select-Object -First 1
if (-not $device) { throw 'no online device in /fleet' }
Write-Output ("device=" + $device.name + " id8=" + $device.id.Substring(0, 8))

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
  if ($reply.error) { throw ("MCP error: " + $reply.error.message) }
  if ($reply.result.isError) { throw ("tool error: " + $reply.result.content[0].text) }
  return $reply.result.content[0].text
}

$createdId = $null
$added = $false
try {
  # 1. The live catalogue, read through the route the page uses.
  $catalogue = Invoke-RestMethod -Uri "$BaseUrl/fleet/$($device.id)/tools" -Headers $headers
  $names = @($catalogue.tools | ForEach-Object { $_.name })
  $sources = @($catalogue.tools | ForEach-Object { $_.source } | Sort-Object -Unique) -join ','
  Write-Output ("live_tools=" + $names.Count + " plugins_list=" + ($names -contains 'plugins.list') + " sources=" + $sources)
  if ($names -notcontains 'plugins.list') { throw 'the live catalogue does not carry plugins.list; is the device on the hub build?' }

  # 2. Install the reference plugin and prove its tool is in the live catalogue.
  Invoke-Mcp 'plugins.add' @{ path = $PluginPath } | Out-Null
  $added = $true
  $catalogue = Invoke-RestMethod -Uri "$BaseUrl/fleet/$($device.id)/tools" -Headers $headers
  $names = @($catalogue.tools | ForEach-Object { $_.name })
  if ($names -notcontains 'hello.greet') { throw 'the live catalogue does not show hello.greet after plugins.add' }
  Write-Output ("after_add=" + $names.Count + " has_greet=" + ($names -contains 'hello.greet'))

  # 3. A token can name that plugin tool, because the device advertises it; then revoke it.
  $body = @{
    name = 'console-plugin-check'
    device_id = $device.id
    tools = @('hello.greet')
    allow_destructive = $false
  } | ConvertTo-Json
  $created = Invoke-RestMethod -Method Post -Uri "$BaseUrl/tokens" -Headers $headers -ContentType 'application/json' -Body $body
  $createdId = $created.id
  Write-Output ("token_created=" + [bool]($created.token -and $created.id))
  Invoke-RestMethod -Method Delete -Uri "$BaseUrl/tokens/$createdId" -Headers $headers | Out-Null
  $createdId = $null
  Write-Output 'token_revoked=True'
  Write-Output 'REAL_CONSOLE_OK'
}
finally {
  if ($createdId) {
    try { Invoke-RestMethod -Method Delete -Uri "$BaseUrl/tokens/$createdId" -Headers $headers | Out-Null } catch { }
  }
  if ($added) {
    try { Invoke-Mcp 'plugins.remove' @{ namespace = 'hello' } | Out-Null } catch { }
  }
}
