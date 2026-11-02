# Hello: the reference MCP plugin for the Summrise capability hub.
# It speaks MCP 2025-06-18 over stdio: one JSON-RPC message per line.
# stdout carries JSON only; diagnostics go to stderr; stdin EOF ends the process.

$ErrorActionPreference = 'Stop'
try {
  [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); [Console]::InputEncoding = [System.Text.UTF8Encoding]::new($false)
} catch { [Console]::Error.WriteLine('hello: UTF-8 console setup failed: {0}', $_.Exception.Message) }

# The manifest's env example is real: set HELLO_GREETING to change the greeting.
$script:Greeting = if ($env:HELLO_GREETING) { $env:HELLO_GREETING } else { 'hi' }

function Send-Line($message) {
  [Console]::Out.WriteLine((ConvertTo-Json -InputObject $message -Compress -Depth 20))
  [Console]::Out.Flush()
}
function Send-Result($id, $result) {
  Send-Line ([ordered]@{ jsonrpc = '2.0'; id = $id; result = $result })
}
function Send-Error($id, [int]$code, [string]$text) {
  Send-Line ([ordered]@{ jsonrpc = '2.0'; id = $id; error = [ordered]@{ code = $code; message = $text } })
}
function Get-TextResult([string]$text, [bool]$isError = $false) {
  $result = [ordered]@{ content = @([ordered]@{ type = 'text'; text = $text }) }
  if ($isError) { $result['isError'] = $true }
  return $result
}
function Invoke-Tool([string]$name, $arguments) {
  switch ($name) {
    'greet' {
      $who = 'world'
      if ($null -ne $arguments -and $null -ne $arguments.PSObject.Properties['name']) {
        $candidate = $arguments.name
        if ($candidate -is [string] -and $candidate.Length -gt 0) { $who = $candidate }
      }
      return Get-TextResult ('{0}, {1}!' -f $script:Greeting, $who)
    }
    'echo' {
      if ($null -eq $arguments -or $null -eq $arguments.PSObject.Properties['text'] -or $arguments.text -isnot [string]) {
        return Get-TextResult 'echo needs a string argument named text.' $true
      }
      return Get-TextResult $arguments.text
    }
    default { return $null }
  }
}
function Invoke-Request($request) {
  $id = $request.id; $params = $request.params
  switch ([string]$request.method) {
    'initialize' {
      # The hub requires this version; the answer advertises tools and nothing else.
      Send-Result $id ([ordered]@{
        protocolVersion = '2025-06-18'
        capabilities = [ordered]@{ tools = [ordered]@{} }
        serverInfo = [ordered]@{ name = 'hello'; version = '0.1.0' }
      })
    }
    'ping' { Send-Result $id ([ordered]@{}) }
    'tools/list' {
      Send-Result $id ([ordered]@{ tools = @(
        [ordered]@{
          name = 'greet'
          description = 'Greet someone by name.'
          inputSchema = [ordered]@{
            type = 'object'; properties = [ordered]@{ name = [ordered]@{ type = 'string'; description = 'Who to greet; defaults to world.' } }
            additionalProperties = $false
          }
          annotations = [ordered]@{ readOnlyHint = $true }
        }
        [ordered]@{
          name = 'echo'
          description = 'Echo the text back unchanged.'
          inputSchema = [ordered]@{
            type = 'object'; properties = [ordered]@{ text = [ordered]@{ type = 'string'; description = 'The text to echo.' } }
            required = [string[]]@('text')
            additionalProperties = $false
          }
          annotations = [ordered]@{ readOnlyHint = $true }
        }
      ) })
    }
    'tools/call' {
      $toolResult = Invoke-Tool ([string]$params.name) $params.arguments
      if ($null -eq $toolResult) {
        Send-Error $id -32602 ('unknown tool: {0}' -f $params.name)
      } else {
        Send-Result $id $toolResult
      }
    }
    default { Send-Error $id -32601 ('method not found: {0}' -f $request.method) }
  }
}

# Read newline-delimited JSON-RPC until stdin closes. A line that is not JSON is
# answered with -32700 and the loop continues; it never takes the server down.
while ($null -ne ($line = [Console]::In.ReadLine())) {
  if ([string]::IsNullOrWhiteSpace($line)) { continue }

  try {
    $request = ConvertFrom-Json -InputObject $line -ErrorAction Stop
  } catch {
    [Console]::Error.WriteLine('hello: invalid JSON: {0}', $_.Exception.Message)
    Send-Error $null -32700 'Parse error'
    continue
  }
  # A notification has no id and gets no answer; initialized is the one expected.
  if ($null -eq $request -or $null -eq $request.PSObject.Properties['id']) {
    if ($null -ne $request -and $request.method -ne 'notifications/initialized') {
      [Console]::Error.WriteLine('hello: ignored notification: {0}', $request.method)
    }
    continue
  }
  try {
    Invoke-Request $request
  } catch {
    [Console]::Error.WriteLine('hello: request failed: {0}', $_.Exception.Message)
    Send-Error $request.id -32603 'Internal error'
  }
}
exit 0
