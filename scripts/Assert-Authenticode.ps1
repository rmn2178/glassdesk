# Verify bytes and trust, not merely the presence of a certificate.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string[]]$Path,
    [Parameter(Mandatory)][ValidateSet('Test', 'Production')][string]$Mode,
    [string]$ProductionThumbprint = '',
    [string]$ReportDirectory = ''
)
$ErrorActionPreference = 'Stop'
$testThumbprint = '3A93532146A10D3B1FA8DEAF4192AD4A232FFCAC'
if ($Mode -eq 'Production' -and ($ProductionThumbprint -cnotmatch '\A[0-9A-Fa-f]{40}\z' -or $ProductionThumbprint -eq $testThumbprint)) {
    throw 'A pinned production certificate thumbprint is required; a test certificate cannot be used.'
}
$store = $null
$added = $false
$reports = [Collections.Generic.List[object]]::new()
try {
    foreach ($file in $Path) {
        $resolved = (Resolve-Path -LiteralPath $file).Path
        Write-Host "Reading Authenticode signature: $resolved"
        $signature = Get-AuthenticodeSignature -LiteralPath $resolved
        $cert = $signature.SignerCertificate
        $row = [ordered]@{
            file = [IO.Path]::GetFileName($resolved)
            sha256 = (Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash
            mode = $Mode
            initialStatus = [string]$signature.Status
            initialMessage = $signature.StatusMessage
            subject = if ($cert) { $cert.Subject } else { $null }
            issuer = if ($cert) { $cert.Issuer } else { $null }
            thumbprint = if ($cert) { $cert.Thumbprint } else { $null }
            status = [string]$signature.Status
            message = $signature.StatusMessage
            stage = 'initial-status'
        }
        $reports.Add($row)
        Write-Host ($row | ConvertTo-Json -Compress)
        if (-not $cert -or $signature.Status -eq 'NotSigned') { throw "$file has no Authenticode signature." }
        if ($signature.Status -in @('HashMismatch', 'NotSupportedFileFormat', 'Incompatible')) {
            throw "Invalid signed file ${file}: $($signature.Status): $($signature.StatusMessage)"
        }
        if ($Mode -eq 'Test') {
            if ($cert.Thumbprint -ne $testThumbprint) { throw "Unexpected SignPath test certificate: $($cert.Thumbprint). Check the certificate in SignPath, not the artifact alone." }
            # Never import an arbitrary signer or change a developer's trust.
            if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') {
                throw 'Test certificate trust may only be installed temporarily on a GitHub-hosted Actions runner.'
            }
            if (-not $store) {
                # CurrentUser\Root can open a Security Warning confirmation,
                # which cannot be answered by an unattended runner. Only this
                # disposable, GitHub-hosted VM may trust the pinned test root.
                $row.stage = 'opening-test-root-store'
                Write-Host 'Opening temporary test trust: LocalMachine\Root (GitHub-hosted runner only).'
                $store = [Security.Cryptography.X509Certificates.X509Store]::new('Root', 'LocalMachine')
                $store.Open([Security.Cryptography.X509Certificates.OpenFlags]::ReadWrite)
                $row.stage = 'finding-test-root'
                Write-Host "Checking pinned test root: $testThumbprint"
                $existing = $store.Certificates.Find(
                    [Security.Cryptography.X509Certificates.X509FindType]::FindByThumbprint, $testThumbprint, $false)
                if ($existing.Count -eq 0) {
                    $row.stage = 'adding-test-root'
                    Write-Host 'Installing the pinned test root temporarily.'
                    $store.Add($cert)
                    $added = $true
                    Write-Host 'Temporary test root installed.'
                }
            }
            $row.stage = 'rechecking-signature'
            Write-Host "Rechecking Authenticode with temporary test trust: $file"
            $signature = Get-AuthenticodeSignature -LiteralPath $resolved
        } elseif ($cert.Thumbprint -eq $testThumbprint -or $cert.Subject -eq $cert.Issuer) {
            throw "$file uses a test/self-signed certificate and cannot be released. Configure the production certificate in SignPath."
        } elseif ($cert.Thumbprint -ne $ProductionThumbprint) {
            throw "Unexpected production signer for ${file}: $($cert.Thumbprint). Confirm the configured production certificate in SignPath."
        }
        $row.status = [string]$signature.Status
        $row.message = $signature.StatusMessage
        $row.stage = 'final-status'
        Write-Host "Final Authenticode status for ${file}: $($signature.Status): $($signature.StatusMessage)"
        if ($signature.Status -ne 'Valid') {
            throw "Authenticode verification failed for ${file}: $($signature.Status): $($signature.StatusMessage). See signature-report.json; do not bypass this check."
        }
        $row.stage = 'complete'
    }
} finally {
    try {
        if ($store) {
            try {
                if ($added) {
                    Write-Host 'Removing the temporary pinned test root.'
                    foreach ($certificate in $store.Certificates.Find(
                        [Security.Cryptography.X509Certificates.X509FindType]::FindByThumbprint, $testThumbprint, $false)) {
                        $store.Remove($certificate)
                    }
                }
            } finally { $store.Close() }
        }
    } finally {
        if ($ReportDirectory) {
            New-Item -ItemType Directory -Path $ReportDirectory -Force | Out-Null
            ConvertTo-Json -InputObject @($reports.ToArray()) -Depth 4 |
                Set-Content -LiteralPath (Join-Path $ReportDirectory 'signature-report.json') -Encoding utf8NoBOM
        }
    }
}
