#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# The Ubuntu Actions bootstrap for the YAML reader pinned in mise.toml.
# No Actions dependency: the lock-sync gate must bootstrap independently.
set -euo pipefail
case "$(uname -s):$(uname -m)" in
  Linux:x86_64) ;;
  *) echo 'Use mise install for this platform; CI bootstrap supports Linux x86_64.' >&2; exit 1 ;;
esac
command -v jq >/dev/null || { echo 'jq must be installed by the runner image.' >&2; exit 1; }
destination="${RUNNER_TEMP:?This bootstrap is for GitHub Actions}/idaptik-yq"
mkdir -p "$destination"
curl --fail --silent --show-error --location --retry 3 \
  https://github.com/mikefarah/yq/releases/download/v4.53.3/yq_linux_amd64 \
  --output "$destination/yq"
printf '%s  %s\n' fa52a4e758c63d38299163fbdd1edfb4c4963247918bf9c1c5d31d84789eded4 "$destination/yq" | sha256sum --check
chmod 755 "$destination/yq"
printf '%s\n' "$destination" >> "${GITHUB_PATH:?}"
