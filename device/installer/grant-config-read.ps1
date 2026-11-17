<#
.SYNOPSIS
  Grants the logged-in host user read access to the device config, and nothing else.

.DESCRIPTION
  <install>\etc\config.yaml is owned by SYSTEM, its inheritance is off, and its DACL names only
  SYSTEM and Administrators. The Electron host runs with the logged-in user's filtered
  (non-elevated) token, so its stat/open of that file is denied and the host's self-check cannot
  tell "denied" from "absent". The installer runs elevated - on the verified silent path through
  the one-shot SYSTEM task, i.e. as the file's owner, which holds WRITE_DAC - and adds exactly one
  ACE: the interactive user's SID, Read only, on that one file.

  The least-privilege rule this script exists to keep:
    * one user SID, never Users, Everyone or a group;
    * Read only - no write, no modify, no take-ownership, no inheritance flags;
    * the one config file, not the etc directory and not the tree;
    * no copy or relocation of the config; its bytes are never opened here (security metadata
      only), so no token can be read, logged or copied by this script;
    * idempotent: re-running the grant leaves one ACE for the SID;
    * a missing config or an unresolvable user SID is logged and exits 0 (nothing to fix - the
      installer neither creates etc nor config.yaml);
    * an icacls refusal is logged and exits 1; the installer records it and continues, and the
      self-check then truthfully reports readable:false.

  The SID comes from the machine's own processes, never from a file or an argument: the owner of
  a running SummriseDesktop.exe, else the owner of explorer.exe, else this process's own user
  when it is not a service SID (S-1-5-18/19/20). On the silent install path the relay's
  StopRuntime has already stopped SummriseDesktop.exe, so explorer.exe supplies the interactive
  user; the setup itself runs as SYSTEM, which is why that fallback is excluded.

.PARAMETER Config
  The config file to grant, normally <install>\etc\config.yaml.

.PARAMETER Revoke
  Remove exactly the ACE this script grants (same SID, grant ACE only). Used by the round's tests
  and available for an uninstall; the uninstaller itself intentionally leaves etc and its ACE
  alone, which is the pre-existing behaviour.

.OUTPUTS
  config-acl: granted <SID> (R) on <path>
  config-acl: absent: no config at <path>; skipped
  config-acl: skipped: no interactive user SID
  config-acl: denied: icacls exit <n> on <path>
  config-acl: revoked <SID> (R) on <path>
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $Config,
    [switch] $Revoke
)

$ErrorActionPreference = 'Stop'

# Service SIDs are never "the host user": they already own or administer the file, and granting one
# would be a no-op at best and a widening at worst.
$ServiceSids = @('S-1-5-18', 'S-1-5-19', 'S-1-5-20')

function Write-ConfigAcl([string] $Line) {
    Write-Output ('config-acl: ' + $Line)
}

function Test-UserSid([string] $Sid) {
    if ([string]::IsNullOrEmpty($Sid)) { return $false }
    if ($ServiceSids -contains $Sid) { return $false }
    # A well-formed SID string; the process owner must be a real account, not a name.
    return ($Sid -match '^S-1-[0-9]+(-[0-9]+)+$')
}

function Get-ProcessOwnerSid([string] $Name) {
    try {
        $proc = Get-CimInstance -ClassName Win32_Process -Filter "Name='$Name'" -ErrorAction SilentlyContinue |
            Select-Object -First 1
        if ($null -eq $proc) { return $null }

        # The direct path: Win32_Process.GetOwnerSid returns the SID the process runs under.
        $ownerSid = Invoke-CimMethod -InputObject $proc -MethodName GetOwnerSid -ErrorAction SilentlyContinue
        if ($null -ne $ownerSid -and $ownerSid.ReturnValue -eq 0 -and (Test-UserSid ([string] $ownerSid.Sid))) {
            return [string] $ownerSid.Sid
        }

        # Fallback for a provider that omits GetOwnerSid: translate the account name.
        $owner = Invoke-CimMethod -InputObject $proc -MethodName GetOwner -ErrorAction SilentlyContinue
        if ($null -ne $owner -and $owner.ReturnValue -eq 0 -and $owner.User) {
            if ($owner.Domain) {
                $account = '{0}\{1}' -f $owner.Domain, $owner.User
            } else {
                $account = [string] $owner.User
            }
            $translated = (New-Object System.Security.Principal.NTAccount($account)).Translate(
                [System.Security.Principal.SecurityIdentifier]).Value
            if (Test-UserSid $translated) { return $translated }
        }
    } catch {
        return $null
    }
    return $null
}

function Resolve-HostUserSid {
    $sid = Get-ProcessOwnerSid 'SummriseDesktop.exe'
    if (Test-UserSid $sid) { return $sid }
    $sid = Get-ProcessOwnerSid 'explorer.exe'
    if (Test-UserSid $sid) { return $sid }
    $self = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    if (Test-UserSid $self) { return $self }
    return $null
}

function Invoke-Icacls([string[]] $IcaclsArgs) {
    # icacls writes its verdict to stdout on success and to stderr on refusal; one merged, bounded
    # line is what the installer's log carries. No file content is involved.
    $lines = & icacls.exe @IcaclsArgs 2>&1
    $code = $LASTEXITCODE
    return [pscustomobject]@{
        Code = $code
        Text = (($lines | ForEach-Object { [string] $_ }) -join ' ')
    }
}

if (-not (Test-Path -LiteralPath $Config -PathType Leaf)) {
    Write-ConfigAcl ('absent: no config at {0}; skipped' -f $Config)
    exit 0
}

$sid = Resolve-HostUserSid
if (-not (Test-UserSid $sid)) {
    Write-ConfigAcl 'skipped: no interactive user SID'
    exit 0
}

if ($Revoke) {
    # /remove:g touches only this SID's grant ACEs - the same ACE this script adds, never a deny
    # ACE and never another account's grant.
    $result = Invoke-Icacls @($Config, '/remove:g', ('*{0}' -f $sid))
    if ($result.Code -eq 0) {
        Write-ConfigAcl ('revoked {0} (R) on {1}' -f $sid, $Config)
        exit 0
    }
    Write-ConfigAcl ('denied: icacls /remove:g exit {0} on {1}: {2}' -f $result.Code, $Config, $result.Text)
    exit 1
}

$result = Invoke-Icacls @($Config, '/grant', ('*{0}:(R)' -f $sid))
if ($result.Code -eq 0) {
    Write-ConfigAcl ('granted {0} (R) on {1}' -f $sid, $Config)
    exit 0
}
Write-ConfigAcl ('denied: icacls exit {0} on {1}: {2}' -f $result.Code, $Config, $result.Text)
exit 1
