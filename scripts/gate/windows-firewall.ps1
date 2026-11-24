# One firewall rule per fixed test path (scripts/gate/test-run.ps1), made from an elevated PowerShell after the first
# gate and after one that adds a test binary: allowed on private networks, blocked on public ones, so the firewall
# asks nothing either way. A fixed copy no current test binary names (one renamed or removed since) goes first, and
# a rule whose path is gone with it. An elevated shell starts in System32, so name the script by its whole path:
#   powershell -ExecutionPolicy Bypass -File <tree>\scripts\gate\windows-firewall.ps1
$ErrorActionPreference = 'Stop'
$dir = Join-Path $PSScriptRoot '..\..\build\test-run'
# The binary ids nextest names now, as test-run.ps1 turns each into its file.
$listed = & {
    $ErrorActionPreference = 'Continue'
    Push-Location (Join-Path $PSScriptRoot '..\..\client')
    try { cargo nextest list --list-type binaries-only --message-format json 2>$null } finally { Pop-Location }
}
if ($LASTEXITCODE -ne 0) { throw 'firewall: cargo nextest could not list the test binaries, so no copy is judged stale' }
$current = @(($listed | Out-String | ConvertFrom-Json).'rust-binaries'.PSObject.Properties.Name | ForEach-Object { ($_ -replace '::|/', '.') + '.exe' })
if ($current.Count -eq 0) { throw 'firewall: cargo nextest listed no test binaries, so no copy is judged stale' }
foreach ($exe in Get-ChildItem $dir -Filter *.exe) {
    if ($exe.Name -notin $current) { Remove-Item -LiteralPath $exe.FullName; Write-Output "firewall: removed copy $($exe.Name)" }
}
foreach ($rule in Get-NetFirewallRule -DisplayName 'Chatkord test *' -ErrorAction SilentlyContinue) {
    $program = ($rule | Get-NetFirewallApplicationFilter).Program
    if (-not (Test-Path -LiteralPath $program)) { $rule | Remove-NetFirewallRule; Write-Output "firewall: removed $program" }
}
foreach ($exe in Get-ChildItem $dir -Filter *.exe) {
    $name = "Chatkord test $($exe.BaseName)"
    Get-NetFirewallRule -DisplayName "$name *" -ErrorAction SilentlyContinue | Remove-NetFirewallRule
    New-NetFirewallRule -DisplayName "$name (private)" -Program $exe.FullName -Direction Inbound -Action Allow -Profile Private, Domain | Out-Null
    New-NetFirewallRule -DisplayName "$name (public)" -Program $exe.FullName -Direction Inbound -Action Block -Profile Public | Out-Null
    Write-Output "firewall: $($exe.FullName)"
}
