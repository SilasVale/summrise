<#
.SYNOPSIS
Prove the hub's plugin path end to end through the deployed console.

.DESCRIPTION
The console's real-device check proves a terminal; this proves the other new half: an operator
session, a device channel, a plugin installed by path, its tools in `tools/list`, a call that
returns the plugin's answer, its log, and its removal. It reads the operator token from its file on
this host and never prints the token, the session token or the tool output beyond the marker.

Run it after the device carries the plugin host and the console is deployed. A failure is a real
deployment failure, not a test failure.

.PARAMETER BaseUrl
The console's loopback address. Default http://127.0.0.1:19000.

.PARAMETER OperatorTokenFile
The file C:\SummriseConsole\run-console.cmd passes to --operator-token-file.

.PARAMETER PluginPath
The plugin directory ON THE DEVICE HOST (the console proxies the call; the device reads the path).
Default C:\SummrisePlugin\hello.

.PARAMETER Namespace
The manifest namespace to expect. Default hello.
#>
param(
  [string]$BaseUrl = 'http://127.0.0.1:19000',
  [string]$OperatorTokenFile = 'C:\SummriseConsole\operator.token',
  [string]$PluginPath = 'C:\SummrisePlugin\hello',
  [string]$Namespace = 'hello'
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
Write-Output ("device=" + $device.name + " id8=" + $device.id.Substring(0, 8) + " online=" + $device.online)

function Invoke-McpRequest([string]$method, $params) {
  $payload = @{
    device_id = $device.id
    request = @{ jsonrpc = '2.0'; id = 1; method = $method; params = $params }
  } | ConvertTo-Json -Depth 16
  $reply = Invoke-RestMethod -Method Post -Uri "$BaseUrl/mcp" -Headers $headers -ContentType 'application/json' -Body $payload
  if ($reply.error) {
    $text = if ($reply.error.message) { $reply.error.message } else { ($reply.error | ConvertTo-Json -Compress) }
    throw "MCP error: $text"
  }
  return $reply.result
}

function Invoke-Tool([string]$name, [hashtable]$arguments) {
  $result = Invoke-McpRequest 'tools/call' @{ name = $name; arguments = $arguments }
  if ($result.isError) { throw ("tool error: " + $result.content[0].text) }
  return $result.content[0].text
}

function Get-ToolNames {
  $list = Invoke-McpRequest 'tools/list' @{}
  return @($list.tools | ForEach-Object { $_.name })
}

function Remove-PluginIfPresent {
  $plugins = (Invoke-Tool 'plugins.list' @{}) | ConvertFrom-Json
  if (@($plugins | Where-Object { $_.namespace -eq $Namespace }).Count -gt 0) {
    Invoke-Tool 'plugins.remove' @{ namespace = $Namespace } | Out-Null
    Write-Output ("removed_stale=" + $Namespace)
  }
}

try {
  Remove-PluginIfPresent

  $added = (Invoke-Tool 'plugins.add' @{ path = $PluginPath }) | ConvertFrom-Json
  Write-Output ("plugin_state=" + $added.plugin.state + " tools=" + $added.plugin.tools)
  if ($added.plugin.state -ne 'running') { throw ("plugin is " + $added.plugin.state + ", not running") }

  $names = Get-ToolNames
  $greet = "$Namespace.greet"
  if ($names -notcontains $greet) { throw "tools/list does not advertise $greet" }
  Write-Output ("tools_list_has=" + $greet + " total=" + $names.Count)

  $answer = Invoke-Tool $greet @{ name = 'Ada' }
  if ($answer -notmatch 'Ada') { throw "greet did not answer with the name; got: $answer" }
  Write-Output ("greet_answer=" + $answer)

  $log = (Invoke-Tool 'plugins.logs' @{ namespace = $Namespace; lines = 20 }) | ConvertFrom-Json
  if ($null -eq $log.lines) { throw 'plugins.logs returned no lines array' }
  Write-Output ("log_lines=" + @($log.lines).Count)

  $removed = (Invoke-Tool 'plugins.remove' @{ namespace = $Namespace }) | ConvertFrom-Json
  if ($removed.plugin.state -ne 'stopped') { throw ("remove left the plugin " + $removed.plugin.state) }
  $namesAfter = Get-ToolNames
  if ($namesAfter -contains $greet) { throw "tools/list still advertises $greet after remove" }
  Write-Output 'REAL_PLUGIN_OK'
}
finally {
  # A failed check must not leave a running child behind on the machine.
  try { Remove-PluginIfPresent } catch { }
}
