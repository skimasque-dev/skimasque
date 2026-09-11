#!/usr/bin/env pwsh
#
# Build and publish the GATEWAY image without GitHub Actions.
#
# The normal path is a `v*` release tag — the release workflow builds and pushes
# `ghcr.io/skimasque-dev/skimasque` for linux/amd64+arm64 and attaches a tarball.
# Use this to publish an unreleased ref, or when Actions minutes have run out.
# Run it from a machine that can compile the workspace, not a gateway host.
#
#   deploy/docker/publish-gateway.ps1                   # push :latest + :sha-<short> to GHCR
#   deploy/docker/publish-gateway.ps1 v0.2.0            # also push :v0.2.0
#   deploy/docker/publish-gateway.ps1 -Mode save        # write a .tar.gz instead of pushing (no registry auth)
#
# GHCR push needs a one-time login with a classic PAT that has write:packages:
#   $env:PAT | docker login ghcr.io -u <github-user> --password-stdin
#
# The image is linux/amd64. For a multi-arch push (amd64+arm64), cut a release
# tag instead, or set up `docker buildx` with QEMU.

[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [string]$Version,

    [ValidateSet('push', 'save')]
    [string]$Mode = $(if ($env:MODE) { $env:MODE } else { 'push' })
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

Set-Location (git rev-parse --show-toplevel)

$image = if ($env:SKIMASQUE_IMAGE) { $env:SKIMASQUE_IMAGE } else { 'ghcr.io/skimasque-dev/skimasque' }
$sha = git rev-parse --short=12 HEAD
$dirty = ''
git diff --quiet
if ($LASTEXITCODE -ne 0) { $dirty = '-dirty' }
$LASTEXITCODE = 0

$tags = @("${image}:latest", "${image}:sha-${sha}${dirty}")
if ($Version) { $tags += "${image}:$Version" }

# `--target gateway`: the published image, never the control-plane binary.
# `--provenance=false`: a plain single-platform image, matching the release
# workflow, so `docker load` is clean.
$buildArgs = @('-f', 'deploy/docker/Dockerfile', '--target', 'gateway', '--platform', 'linux/amd64', '--provenance=false')
foreach ($t in $tags) { $buildArgs += @('-t', $t) }

Write-Host ">> building $image (${sha}${dirty})"
docker build @buildArgs .

if ($Mode -eq 'save') {
    $out = "skimasque-${sha}${dirty}.tar.gz"
    # `docker save -o` writes an uncompressed tar; gzip it via .NET so this
    # doesn't depend on a gzip binary being on PATH (it usually isn't on Windows).
    $tar = "$out.tmp"
    docker save "${image}:latest" -o $tar
    try {
        $in = [System.IO.File]::OpenRead($tar)
        $fs = [System.IO.File]::Create($out)
        $gz = [System.IO.Compression.GZipStream]::new($fs, [System.IO.Compression.CompressionLevel]::Optimal)
        $in.CopyTo($gz)
    }
    finally {
        if ($gz) { $gz.Dispose() }
        if ($fs) { $fs.Dispose() }
        if ($in) { $in.Dispose() }
        Remove-Item $tar -ErrorAction SilentlyContinue
    }
    $size = '{0:N1} MB' -f ((Get-Item $out).Length / 1MB)
    Write-Host ''
    Write-Host ">> wrote $out ($size)"
    Write-Host '   copy it to the gateway host, then:'
    Write-Host "     docker load < $out"
    Write-Host '     deploy/docker/run-gateway.sh          # or restart your gateway unit / pod'
}
else {
    foreach ($t in $tags) {
        Write-Host ">> pushing $t"
        docker push $t
    }
    Write-Host ''
    Write-Host '>> on the gateway host, pick up the new image:'
    Write-Host '     deploy/docker/run-gateway.sh          # re-run pulls and replaces the container'
    Write-Host "   (systemd: docker pull ${image}:latest && systemctl restart skimasque-gateway)"
}
