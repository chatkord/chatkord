# Runs a test binary from a path that does not change when it is rebuilt, so Windows' firewall is asked about each path
# once (scripts/gate/windows-firewall.ps1): a test binary's name carries a hash, and the engine's sockets have no
# loopback-only mode. The path is nextest's binary id, unique in the workspace where a file name is not (every crate's
# integration binary is `suite`); outside nextest, which names none, the binary runs where cargo built it.
param([Parameter(Mandatory)][string]$Binary, [Parameter(ValueFromRemainingArguments)][string[]]$Rest)
$ErrorActionPreference = 'Stop'
if (-not $env:NEXTEST_BINARY_ID) { & $Binary @Rest; exit $LASTEXITCODE }
$dir = Join-Path $PSScriptRoot '..\..\build\test-run'
New-Item -ItemType Directory -Force $dir | Out-Null
$fixed = Join-Path $dir (($env:NEXTEST_BINARY_ID -replace '::|/', '.') + '.exe') # `cli::bin/chatkord` too
$built = Get-Item $Binary
$have = Get-Item $fixed -ErrorAction SilentlyContinue
# nextest starts several processes per binary at once (its two listings, then one per test). The fixed file is only
# ever replaced by one rename, so another process finds the old bytes or the new, never no file; one that is running
# cannot be replaced, and holds the same bytes.
if (-not $have -or $have.Length -ne $built.Length -or $have.LastWriteTimeUtc -ne $built.LastWriteTimeUtc) {
    $partial = "$fixed.$PID"
    Copy-Item $Binary $partial
    Add-Type -Namespace TestRun -Name Native -MemberDefinition @'
[DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
public static extern bool MoveFileEx(string from, string to, int flags);
'@
    # MOVEFILE_REPLACE_EXISTING
    if (-not [TestRun.Native]::MoveFileEx($partial, $fixed, 1)) { Remove-Item $partial -ErrorAction SilentlyContinue }
}
& $fixed @Rest
exit $LASTEXITCODE
