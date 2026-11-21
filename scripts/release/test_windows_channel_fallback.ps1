# Windows installer channel and URL regression. The local HTTP fixture serves
# both the legacy custom-channel layout and the fork's GitHub Releases layout.
# The Windows runtime battery separately verifies the real executable.

$ErrorActionPreference = 'Stop'

$repo = (Get-Location).Path
$scratch = Join-Path ([IO.Path]::GetTempPath()) ("prime-agent-channel-fallback-{0}" -f [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch | Out-Null

# The Python interpreter for the local http server (the install e2e's pattern).
$py = if (Get-Command python3 -ErrorAction SilentlyContinue) { 'python3' } else { 'python' }

$stableVersion = '9.9.9'
$betaVersion = '9.9.9-beta.1'
$platform = 'win32-x64'

# The stub artifact the beta release prefix serves: a tarball whose payload
# is a placeholder exe (the install only checks the payload exists).
$payload = Join-Path $scratch 'payload'
New-Item -ItemType Directory -Path $payload | Out-Null
Set-Content -LiteralPath (Join-Path $payload 'prime-agent.exe') -Value 'stub payload for the channel-fallback regression test'
$artifactFile = "prime-agent-$betaVersion-$platform.tar.gz"
$artifact = Join-Path $scratch $artifactFile
& tar -czf $artifact -C $payload prime-agent.exe
if ($LASTEXITCODE -ne 0) { throw 'the stub artifact build failed (tar)' }
$artifactSha = (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLower()

# Start with a missing stable artifact and an available beta artifact.
$channel = Join-Path $scratch 'channel'
$releaseDir = Join-Path $channel "releases\v$betaVersion"
New-Item -ItemType Directory -Path $releaseDir -Force | Out-Null
Move-Item -LiteralPath $artifact -Destination $releaseDir
Set-Content -LiteralPath (Join-Path $releaseDir 'SHA256SUMS') -Value "$artifactSha  $artifactFile"
Set-Content -LiteralPath (Join-Path $channel 'stable') -Value $stableVersion -NoNewline
Set-Content -LiteralPath (Join-Path $channel 'latest.json') -Value ('{"version": "v' + $stableVersion + '", "binaries": [], "binaries_v2": []}')
Set-Content -LiteralPath (Join-Path $channel 'beta') -Value $betaVersion -NoNewline
$betaRow = '{"platform": "' + $platform + '", "file": "' + $artifactFile + '", "sha256": "' + $artifactSha + '"}'
Set-Content -LiteralPath (Join-Path $channel 'beta.json') -Value ('{"version": "v' + $betaVersion + '", "binaries": [' + $betaRow + '], "binaries_v2": [' + $betaRow + ']}')

# One install run under a given channel knob, with the transcript captured.
# It dials the channel server's $baseUrl, which the try block below learns
# from the server itself.
function Invoke-Installer {
    param(
        [string]$ChannelKnob,
        [string]$Prefix,
        [string]$Installer = (Join-Path $repo 'install.ps1'),
        [string]$DownloadBase = $baseUrl,
        [string]$VersionPin
    )
    Remove-Item -Path 'Env:PRIME_AGENT_DOWNLOAD_BASE_URL', 'Env:PRIME_AGENT_RELEASE_CHANNEL', 'Env:PRIME_AGENT_RUST_PREFIX', 'Env:PRIME_AGENT_ALLOW_HTTP', 'Env:PRIME_AGENT_VERSION' -ErrorAction SilentlyContinue
    if ($DownloadBase) { $env:PRIME_AGENT_DOWNLOAD_BASE_URL = $DownloadBase }
    $env:PRIME_AGENT_ALLOW_HTTP = '1'
    $env:PRIME_AGENT_RUST_PREFIX = $Prefix
    if ($ChannelKnob) { $env:PRIME_AGENT_RELEASE_CHANNEL = $ChannelKnob }
    if ($VersionPin) { $env:PRIME_AGENT_VERSION = $VersionPin }
    $lines = @(& pwsh -NoProfile -File $Installer 2>&1 | ForEach-Object { "$_" })
    return [pscustomobject]@{ Lines = $lines; Exit = $LASTEXITCODE }
}

# The server picks and holds its own port (http.server on port 0: the OS
# hands the port out atomically, so no other process can claim it) and
# announces it on stdout; -u flushes the announcement immediately, and the
# test reads it back from the log.
$serverLog = Join-Path $scratch 'channel-server.log'
$server = Start-Process -FilePath $py -ArgumentList '-u','-m','http.server','0','--bind','127.0.0.1','--directory',$channel -PassThru -WindowStyle Hidden -RedirectStandardOutput $serverLog
# install.ps1 writes the User PATH (the PATH-parity flow) and installs uv
# when the machine has none (the astral route); the harness strips exactly
# its own scratch-prefixed PATH entries in the cleanup below.
# THE UV INSTALL IS ISOLATED (the reviewers' finding): ownership of a file
# in the shared ~/.local/bin is unprovable, so the harness never lets the
# installer touch the shared dir - install.ps1 honors
# PRIME_AGENT_UV_BIN_DIR and the harness points it at its scratch.
# The kernel pre-warm's writes (the venv, uv's cache, uv's pythons) would
# land in the real user profile too; the harness steers all of them into
# its own scratch dir through the product's override knobs and restores
# the caller's values in the cleanup (the discipline install.ps1 itself
# runs).
$callerKernelVenv = $env:PRIME_AGENT_KERNEL_VENV
$callerUvCacheDir = $env:UV_CACHE_DIR
$callerUvPythonDir = $env:UV_PYTHON_INSTALL_DIR
$callerUvBinDir = $env:PRIME_AGENT_UV_BIN_DIR
try {
    $env:PRIME_AGENT_KERNEL_VENV = Join-Path $scratch 'kernel-venv'
    $env:UV_CACHE_DIR = Join-Path $scratch 'uv-cache'
    $env:UV_PYTHON_INSTALL_DIR = Join-Path $scratch 'uv-python'
    $env:PRIME_AGENT_UV_BIN_DIR = Join-Path $scratch 'uv-bin'
    # The port the server itself announced, then readiness is it answering
    # a request for this test's own channel (beta.json): bounded deadlines,
    # and a dead child fails fast instead of hanging the installer.
    $deadline = (Get-Date).AddSeconds(30)
    $port = $null
    while ($null -eq $port) {
        if ($server.HasExited) { throw "the local channel server exited early" }
        if ((Get-Date) -gt $deadline) { throw "the local channel server did not announce its port within 30s" }
        $announced = Select-String -LiteralPath $serverLog -Pattern 'port (\d+)' -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($announced) { $port = [int]$announced.Matches[0].Groups[1].Value }
        if ($null -eq $port) { Start-Sleep -Milliseconds 100 }
    }
    $baseUrl = "http://127.0.0.1:$port"
    $ready = $false
    while (-not $ready) {
        if ($server.HasExited) { throw "the local channel server exited early (port $port)" }
        if ((Get-Date) -gt $deadline) { throw "the local channel server did not answer on $baseUrl within 30s" }
        try {
            $probe = Invoke-WebRequest -Uri "$baseUrl/beta.json" -TimeoutSec 2
            if ($probe.StatusCode -eq 200) { $ready = $true }
        } catch {
            Start-Sleep -Milliseconds 100
        }
    }

    foreach ($selection in @('default', 'stable')) {
        $prefix = Join-Path $scratch "prefix-missing-$selection"
        $knob = if ($selection -eq 'default') { $null } else { $selection }
        $run = Invoke-Installer -ChannelKnob $knob -Prefix $prefix
        $transcript = $run.Lines -join [Environment]::NewLine
        if ($run.Exit -eq 0 -or $transcript -notmatch [regex]::Escape('no artifact row for platform win32-x64 in the stable manifest')) {
            $run.Lines | Write-Host
            throw "$selection must refuse the missing stable artifact"
        }
        if (Test-Path (Join-Path $prefix 'share\prime-agent\prime-agent.exe')) {
            throw "$selection unexpectedly published a payload"
        }
    }

    # Explicit beta succeeds independently of the incomplete stable release.
    $prefixBeta = Join-Path $scratch 'prefix-beta'
    $runBeta = Invoke-Installer -ChannelKnob 'beta' -Prefix $prefixBeta
    if ($runBeta.Exit -ne 0) {
        $runBeta.Lines | Write-Host
        throw 'the explicit beta install failed'
    }
    $marker = Get-Content -LiteralPath (Join-Path $prefixBeta 'share\prime-agent\.prime-agent-install') -Raw
    if ($marker -ne "install-rust.sh channel beta`nversion $betaVersion") { throw "unexpected beta marker: $marker" }

    # Publish stable and verify the custom-base default selects its artifact.
    $stableFile = "prime-agent-$stableVersion-$platform.tar.gz"
    $stableDir = Join-Path $channel "releases\v$stableVersion"
    New-Item -ItemType Directory -Path $stableDir -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $releaseDir $artifactFile) -Destination (Join-Path $stableDir $stableFile)
    Set-Content -LiteralPath (Join-Path $stableDir 'SHA256SUMS') -Value "$artifactSha  $stableFile"
    $stableRow = '{"platform": "' + $platform + '", "file": "' + $stableFile + '", "sha256": "' + $artifactSha + '"}'
    Set-Content -LiteralPath (Join-Path $channel 'latest.json') -Value ('{"version": "v' + $stableVersion + '", "binaries": [' + $stableRow + '], "binaries_v2": [' + $stableRow + ']}')
    $prefixStable = Join-Path $scratch 'prefix-stable'
    $runStable = Invoke-Installer -ChannelKnob $null -Prefix $prefixStable
    if ($runStable.Exit -ne 0) {
        $runStable.Lines | Write-Host
        throw 'the default stable install failed'
    }
    $marker = Get-Content -LiteralPath (Join-Path $prefixStable 'share\prime-agent\.prime-agent-install') -Raw
    if ($marker -ne "install-rust.sh channel stable`nversion $stableVersion") { throw "unexpected stable marker: $marker" }
    foreach ($prefix in @($prefixBeta, $prefixStable)) {
        if (-not (Test-Path (Join-Path $prefix 'share\prime-agent\prime-agent.exe') -PathType Leaf)) { throw "missing payload in $prefix" }
    }

    # Redirect only the fork's built-in origin to the fixture, not the override:
    # this exercises the unstamped default, including both release URL shapes.
    $installerSource = Get-Content -LiteralPath (Join-Path $repo 'install.ps1') -Raw
    $forkInstaller = Join-Path $scratch 'install-fork.ps1'
    Set-Content -LiteralPath $forkInstaller -Value ($installerSource.Replace('https://github.com/csmoke66/prime-agent/releases', "$baseUrl/fork/releases"))
    $forkPrefix = Join-Path $scratch 'prefix-fork'
    $forkArgs = @{ Installer = $forkInstaller; DownloadBase = ''; Prefix = $forkPrefix }
    $missingFork = Invoke-Installer @forkArgs
    if ($missingFork.Exit -eq 0 -or ($missingFork.Lines -join ' ') -notmatch 'no Windows build has been published on the fork.*push main from GitHub Desktop') {
        $missingFork.Lines | Write-Host
        throw 'the fork default did not explain how to publish its missing Windows build'
    }
    $betaFork = Invoke-Installer @forkArgs -ChannelKnob beta -VersionPin $stableVersion
    if ($betaFork.Exit -eq 0 -or ($betaFork.Lines -join ' ') -notmatch 'the fork publishes only the stable Windows channel') {
        $betaFork.Lines | Write-Host
        throw 'beta must be refused on the fork, even with an exact version pin'
    }

    $forkLatest = Join-Path $channel 'fork\releases\latest\download'
    $forkTag = Join-Path $channel "fork\releases\download\windows-v$stableVersion"
    New-Item -ItemType Directory -Path $forkLatest, $forkTag -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $channel 'stable'), (Join-Path $channel 'latest.json') -Destination $forkLatest
    Copy-Item -LiteralPath (Join-Path $stableDir $stableFile), (Join-Path $stableDir 'SHA256SUMS') -Destination $forkTag
    $stableFork = Invoke-Installer @forkArgs
    if ($stableFork.Exit -ne 0 -or -not (Test-Path (Join-Path $forkPrefix 'share\prime-agent\prime-agent.exe') -PathType Leaf)) {
        $stableFork.Lines | Write-Host
        throw 'the fork default must read the latest metadata and download the versioned payload'
    }

    # Pins must bypass latest metadata but still verify the versioned SHA256SUMS.
    Remove-Item -LiteralPath $forkLatest -Recurse
    $forkArgs.Prefix = Join-Path $scratch 'prefix-fork-pin'
    $forkSums = Join-Path $forkTag 'SHA256SUMS'
    Set-Content -LiteralPath $forkSums -Value "$('0' * 64)  $stableFile"
    $corruptPin = Invoke-Installer @forkArgs -VersionPin "v$stableVersion"
    if ($corruptPin.Exit -eq 0 -or ($corruptPin.Lines -join ' ') -notmatch 'checksum mismatch for .*: the download is corrupt' -or (Test-Path $forkArgs.Prefix)) {
        $corruptPin.Lines | Write-Host
        throw 'the pinned fork install must reject a corrupt versioned payload before publishing'
    }
    Copy-Item -LiteralPath (Join-Path $stableDir 'SHA256SUMS') -Destination $forkTag -Force
    $pinnedFork = Invoke-Installer @forkArgs -VersionPin "v$stableVersion"
    if ($pinnedFork.Exit -ne 0 -or -not (Test-Path (Join-Path $forkArgs.Prefix 'share\prime-agent\prime-agent.exe') -PathType Leaf)) {
        $pinnedFork.Lines | Write-Host
        throw 'an exact fork version must install from its tag without reading latest metadata'
    }

    # The R2 renderer stamps these assignments with a legacy base and channel.
    $renderedInstaller = Join-Path $scratch 'install-rendered-beta.ps1'
    $renderedSource = $installerSource.Replace("`$DownloadBaseUrlDefault = ''", "`$DownloadBaseUrlDefault = '$baseUrl'").Replace("`$ReleaseChannelDefault = 'stable'", "`$ReleaseChannelDefault = 'beta'")
    Set-Content -LiteralPath $renderedInstaller -Value $renderedSource
    $renderedPrefix = Join-Path $scratch 'prefix-rendered-beta'
    $renderedBeta = Invoke-Installer -Installer $renderedInstaller -DownloadBase '' -Prefix $renderedPrefix
    if ($renderedBeta.Exit -ne 0) {
        $renderedBeta.Lines | Write-Host
        throw 'the rendered legacy beta installer failed'
    }
    $marker = Get-Content -LiteralPath (Join-Path $renderedPrefix 'share\prime-agent\.prime-agent-install') -Raw
    if ($marker -ne "install-rust.sh channel beta`nversion $betaVersion") { throw "unexpected rendered beta marker: $marker" }

    Write-Host "WIN_CHANNEL_SELECTION custom-stable=$stableVersion explicit-and-rendered-beta=$betaVersion; fork stable and pinned installs verified"
} finally {
    Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue
    # The User PATH: strip ONLY the entries this test added - the ones
    # under its own scratch dir - from the CURRENT registry value, never
    # a stale snapshot restore (a whole-value overwrite would clobber any
    # external PATH change made while the test ran) and never through
    # [Environment]::SetEnvironmentVariable (it flattens a REG_EXPAND_SZ
    # Path to plain REG_SZ with this run's expansion frozen in). The raw
    # value rides out with its registry kind intact; a Path this test
    # created from nothing is deleted again; a Path it never touched is
    # not rewritten at all. A cleanup failure is recorded and the
    # remaining steps still run (the loud tail below reports it).
    $pathCleanFailed = $false
    $envKey = $null
    try {
        $envKey = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
        $rawUserPath = $envKey.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        $rawUserKind = [Microsoft.Win32.RegistryValueKind]::ExpandString
        if ($envKey.GetValueNames() -contains 'Path') {
            $rawUserKind = $envKey.GetValueKind('Path')
        }
        $allEntries = @($rawUserPath -split ';')
        $keptEntries = @($allEntries | Where-Object { -not $_.Trim().StartsWith($scratch, [System.StringComparison]::OrdinalIgnoreCase) })
        if ($keptEntries.Count -lt $allEntries.Count) {
            if ($keptEntries.Count -gt 0) {
                $envKey.SetValue('Path', ($keptEntries -join ';'), $rawUserKind)
            } else {
                $envKey.DeleteValue('Path', $false)
            }
        }
    } catch {
        $pathCleanFailed = $true
    } finally {
        if ($envKey) { $envKey.Close() }
    }
    # The uv install needs no cleanup: it never left the scratch dir (the
    # PRIME_AGENT_UV_BIN_DIR knob above steered the installer), and the
    # user's ~/.local/bin was never touched at all.
    $env:PRIME_AGENT_KERNEL_VENV = $callerKernelVenv
    $env:UV_CACHE_DIR = $callerUvCacheDir
    $env:UV_PYTHON_INSTALL_DIR = $callerUvPythonDir
    $env:PRIME_AGENT_UV_BIN_DIR = $callerUvBinDir
    Remove-Item -Recurse -Force $scratch -ErrorAction SilentlyContinue
    if ($pathCleanFailed) {
        throw 'could not clean the user PATH entries this test added'
    }
}
