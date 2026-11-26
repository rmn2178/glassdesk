# Offline policy tests: no account, key, network, or certificate-store changes.
$ErrorActionPreference = 'Stop'
$modeScript = Join-Path $PSScriptRoot 'Get-ReleaseSigningMode.ps1'
$verifyScript = Join-Path $PSScriptRoot 'Assert-Authenticode.ps1'
$root = Join-Path ([IO.Path]::GetTempPath()) "nosih-signing-test-$([guid]::NewGuid())"
$testPin = '3A93532146A10D3B1FA8DEAF4192AD4A232FFCAC'
$prodPin = '1111111111111111111111111111111111111111'
$savedActions = $env:GITHUB_ACTIONS
$savedRunner = $env:RUNNER_ENVIRONMENT
$state = @{ status = 'Valid'; thumbprint = $prodPin; subject = 'CN=Publisher'; issuer = 'CN=Issuer'; missing = $false }
function Get-AuthenticodeSignature {
    param($LiteralPath)
    [pscustomobject]@{
        Status = $state.status; StatusMessage = 'Native verification fixture detail'
        SignerCertificate = if ($state.missing) { $null } else {
            [pscustomobject]@{ Thumbprint = $state.thumbprint; Subject = $state.subject; Issuer = $state.issuer }
        }
    }
}
Set-Item Function:Get-AuthenticodeSignature -Value ((Get-Command Get-AuthenticodeSignature -CommandType Function).ScriptBlock.GetNewClosure())
function Assert-Rejected([string]$Name, [scriptblock]$Action, [string]$Message) {
    try { & $Action | Out-Null }
    catch {
        if ($_.Exception.Message -notlike $Message) { throw }
        Write-Host "PASS: $Name"
        return
    }
    throw "Failed to reject: $Name"
}
try {
    $env:GITHUB_ACTIONS = ''
    $env:RUNNER_ENVIRONMENT = ''
    New-Item -ItemType Directory -Path $root | Out-Null
    $fixture = Join-Path $root 'fixture.exe'
    Set-Content -LiteralPath $fixture -Value 'test-owned bytes only'
    foreach ($script in @($modeScript, $verifyScript, $PSCommandPath)) {
        $tokens = $null; $errors = $null
        $null = [Management.Automation.Language.Parser]::ParseFile($script, [ref]$tokens, [ref]$errors)
        if ($errors) { throw ($errors | Out-String) }
    }
    $common = @{ Version = '0.1.17'; RefName = 'v0.1.17'; Organization = 'configured'; Project = 'Nosih'; TokenPresent = $true }
    $lane = & $modeScript -EventName workflow_dispatch @common -Policy release-signing
    if ($lane.Mode -ne 'Test' -or $lane.Policy -ne 'test-signing') { throw 'Manual runs must always be tests.' }
    Write-Host 'PASS: manual run uses only test-signing, even with production configured'
    $lane = & $modeScript -EventName push -Version 0.1.17 -RefName v0.1.17
    if ($lane.Mode -ne 'Unsigned') { throw 'Unsigned release compatibility lost.' }
    Write-Host 'PASS: absent organization preserves unsigned tag releases'
    $lane = & $modeScript -EventName push @common -Policy release-signing -ProductionThumbprint $prodPin
    if ($lane.Mode -ne 'Production') { throw 'Production lane missing.' }
    Write-Host 'PASS: configured production tag uses production lane'
    Assert-Rejected 'test policy on public release' { & $modeScript -EventName push @common -Policy test-signing } '*public release requires*'
    Assert-Rejected 'unknown policy on public release' { & $modeScript -EventName push @common -Policy typo } '*public release requires*'
    Assert-Rejected 'missing production pin' { & $modeScript -EventName push @common -Policy release-signing } '*production certificate thumbprint*'
    Assert-Rejected 'test certificate as production pin' { & $modeScript -EventName push @common -Policy release-signing -ProductionThumbprint $testPin } '*production certificate thumbprint*'
    Assert-Rejected 'test without configuration' { & $modeScript -EventName workflow_dispatch -Version 0.1.17 } '*Configure SIGNPATH*'
    Assert-Rejected 'tag mismatch' { & $modeScript -EventName push -Version 0.1.17 -RefName v0.1.16 } '*does not match*'
    & $verifyScript -Mode Production -ProductionThumbprint $prodPin -Path $fixture -ReportDirectory $root
    $report = @(Get-Content (Join-Path $root 'signature-report.json') -Raw | ConvertFrom-Json)
    if ($report[0].sha256 -ne (Get-FileHash $fixture).Hash -or $report[0].status -ne 'Valid') { throw 'Verification report lost hash/status.' }
    Write-Host 'PASS: expected valid production signer records hash and status'
    foreach ($status in @('UnknownError', 'NotTrusted', 'HashMismatch', 'NotSigned', 'Incompatible')) {
        $state.status = $status
        $expectedMessage = if ($status -eq 'NotSigned') { '*no Authenticode signature*' } else { "*${status}: Native verification fixture detail*" }
        Assert-Rejected "production $status" { & $verifyScript -Mode Production -ProductionThumbprint $prodPin -Path $fixture -ReportDirectory $root } $expectedMessage
        $report = @(Get-Content (Join-Path $root 'signature-report.json') -Raw | ConvertFrom-Json)
        if ($report[0].initialStatus -ne $status -or $report[0].initialMessage -ne 'Native verification fixture detail') { throw 'Failure diagnostics missing.' }
    }
    $state.status = 'Valid'
    $state.thumbprint = $testPin
    Assert-Rejected 'valid but test-signed production artifact' { & $verifyScript -Mode Production -ProductionThumbprint $prodPin -Path $fixture } '*test/self-signed*'
    Assert-Rejected 'test trust on a developer machine' { & $verifyScript -Mode Test -Path $fixture } '*GitHub-hosted*'
    $state.status = 'HashMismatch'
    Assert-Rejected 'modified test artifact before trust changes' { & $verifyScript -Mode Test -Path $fixture } '*Invalid signed file*'
    $state.status = 'Valid'; $state.thumbprint = $prodPin; $state.subject = $state.issuer
    Assert-Rejected 'self-signed production artifact' { & $verifyScript -Mode Production -ProductionThumbprint $prodPin -Path $fixture } '*test/self-signed*'
    $state.subject = 'CN=Publisher'; $state.thumbprint = '2222222222222222222222222222222222222222'
    Assert-Rejected 'unexpected production publisher' { & $verifyScript -Mode Production -ProductionThumbprint $prodPin -Path $fixture } '*Unexpected production signer*'
    Assert-Rejected 'unexpected test certificate' { & $verifyScript -Mode Test -Path $fixture } '*Unexpected SignPath test certificate*'
    $state.missing = $true
    Assert-Rejected 'missing signer' { & $verifyScript -Mode Production -ProductionThumbprint $prodPin -Path $fixture } '*no Authenticode signature*'
    $workflow = Get-Content (Join-Path $PSScriptRoot '../.github/workflows/release.yml') -Raw
    foreach ($name in @('Prepare release files', 'Create draft release')) {
        if ($workflow -notmatch ("- name: " + [regex]::Escape($name) + "\r?\n        if: env.SIGNING_MODE != 'Test'")) {
            throw "$name must exclude the test lane."
        }
    }
    if ($workflow -notmatch "- name: Sign the final production installer for the updater\r?\n        if: env.SIGNING_MODE == 'Production'") {
        throw 'Test lane must never generate an updater signature.'
    }
    Write-Host 'PASS: tests cannot prepare release files, updater signatures or draft releases'
    # A native verification call can wait on Windows UI or certificate services.
    # Bound every step containing one, independently of SignPath approval waits.
    foreach ($entry in @(
        @{ Name = 'Verify both signed programs'; Minutes = 5 },
        @{ Name = 'Rebuild the installer around the signed programs'; Minutes = 10 },
        @{ Name = 'Verify and use the signed installer'; Minutes = 5 }
    )) {
        $block = "- name: " + [regex]::Escape($entry.Name) + "\r?\n        if: env.SIGNING_MODE != 'Unsigned'\r?\n        timeout-minutes: " + $entry.Minutes + "\r?\n"
        if ($workflow -notmatch $block) { throw "$($entry.Name) must have a bounded timeout." }
    }
    Write-Host 'PASS: native verification steps have bounded timeouts'
    $verifier = Get-Content $verifyScript -Raw
    if (-not $verifier.Contains("::new('Root', 'LocalMachine')") -or $verifier.Contains("::new('Root', 'CurrentUser')")) {
        throw 'Hosted test trust must avoid the interactive CurrentUser root store.'
    }
    Write-Host 'PASS: hosted test trust avoids the interactive CurrentUser root store'
} finally {
    $env:GITHUB_ACTIONS = $savedActions
    $env:RUNNER_ENVIRONMENT = $savedRunner
    # root is a newly created, test-owned absolute temp directory.
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
}
