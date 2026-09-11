#!/usr/bin/env bash
#
# Build and publish the GATEWAY image without GitHub Actions.
#
# The normal path is a `v*` release tag — the release workflow builds and pushes
# `ghcr.io/skimasque-dev/skimasque` for linux/amd64+arm64 and attaches a tarball.
# Use this to publish an unreleased ref, or when Actions minutes have run out.
# Run it from a machine that can compile the workspace, not a gateway host.
#
#   deploy/docker/publish-gateway.sh                  # push :latest + :sha-<short> to GHCR
#   deploy/docker/publish-gateway.sh v0.2.0           # also push :v0.2.0
#   MODE=save deploy/docker/publish-gateway.sh        # write a .tar.gz instead of pushing (no registry auth)
#
# GHCR push needs a one-time login with a classic PAT that has write:packages:
#   echo "$PAT" | docker login ghcr.io -u <github-user> --password-stdin
#
# The image is linux/amd64. For a multi-arch push (amd64+arm64), cut a release
# tag instead, or set up `docker buildx` with QEMU and build with
# `--platform linux/amd64,linux/arm64 --push`.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

IMAGE="${SKIMASQUE_IMAGE:-ghcr.io/skimasque-dev/skimasque}"
SHA="$(git rev-parse --short=12 HEAD)"
DIRTY=""
git diff --quiet || DIRTY="-dirty"

TAGS=("${IMAGE}:latest" "${IMAGE}:sha-${SHA}${DIRTY}")
[ "${1:-}" ] && TAGS+=("${IMAGE}:$1")

# `--target gateway`: the published image, never the control-plane binary.
# `--provenance=false`: a plain single-platform image, matching the release
# workflow, so `docker load` is clean.
build_args=(-f deploy/docker/Dockerfile --target gateway --platform linux/amd64 --provenance=false)
for t in "${TAGS[@]}"; do build_args+=(-t "$t"); done

echo ">> building ${IMAGE} (${SHA}${DIRTY})"
docker build "${build_args[@]}" .

if [ "${MODE:-push}" = "save" ]; then
  out="skimasque-${SHA}${DIRTY}.tar.gz"
  docker save "${IMAGE}:latest" | gzip > "$out"
  echo
  echo ">> wrote $out ($(du -h "$out" | cut -f1))"
  echo "   copy it to the gateway host, then:"
  echo "     docker load < $out"
  echo "     deploy/docker/run-gateway.sh          # or restart your gateway unit / pod"
else
  for t in "${TAGS[@]}"; do
    echo ">> pushing $t"
    docker push "$t"
  done
  echo
  echo ">> on the gateway host, pick up the new image:"
  echo "     deploy/docker/run-gateway.sh          # re-run pulls and replaces the container"
  echo "   (systemd: docker pull ${IMAGE}:latest && systemctl restart skimasque-gateway)"
fi
