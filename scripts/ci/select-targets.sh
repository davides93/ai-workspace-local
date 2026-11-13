#!/usr/bin/env bash
# Decides which release targets .github/workflows/release.yml builds, and prints them as GitHub
# Actions outputs (JSON matrices for the jobs). One place, testable without GitHub:
#
#   scripts/ci/select-targets.sh --self-test
#   INPUT_TARGETS=linux-arm64,cuda scripts/ci/select-targets.sh        # prints the outputs
#
# Environment:
#   INPUT_TARGETS  the `targets` input of workflow_dispatch (comma separated; empty = not given)
#   VAR_TARGETS    the repository variable AIW_RELEASE_TARGETS (used when the input is empty)
#   GITHUB_OUTPUT  where the outputs are appended (stdout when unset)
#
# With neither the input nor the variable the selection is `default`: exactly the targets that a
# release published before the arm64 and CUDA jobs existed (Windows x64 and macOS universal).
set -euo pipefail

# Individual targets. A group is a space separated list of targets (or of other groups).
GROUPS_DEFAULT="windows-x64 macos"
GROUPS_ARM64="linux-arm64 windows-arm64 daemon-linux-arm64 daemon-windows-arm64"
GROUPS_CUDA="cuda-linux cuda-windows"
VALID="windows-x64 macos linux-arm64 windows-arm64 daemon-linux-arm64 daemon-windows-arm64 cuda-linux cuda-windows"

expand() {
  local out="" t
  for t in $(printf '%s' "$1" | tr 'A-Z;' 'a-z,' | tr ', ' '\n\n' | grep -v '^$' || true); do
    case "$t" in
      default) out="$out $GROUPS_DEFAULT" ;;
      arm64) out="$out $GROUPS_ARM64" ;;
      cuda) out="$out $GROUPS_CUDA" ;;
      extras) out="$out $GROUPS_ARM64 $GROUPS_CUDA" ;;
      all) out="$out $GROUPS_DEFAULT $GROUPS_ARM64 $GROUPS_CUDA" ;;
      *)
        if [[ " $VALID " == *" $t "* ]]; then
          out="$out $t"
        else
          echo "::warning::target di release sconosciuto, ignorato: $t (validi: default, arm64, cuda, extras, all, $VALID)" >&2
        fi
        ;;
    esac
  done
  printf '%s\n' $out | sort -u | tr '\n' ' '
}

# The catalogue: what each target builds. `optional` legs never block the release.
catalogue() {
  cat <<'JSON'
{
  "desktop": [
    { "id": "windows-x64", "name": "Windows x64", "os": "windows-latest", "args": "--features computer-use --bundles nsis", "targets": "", "arm": false, "optional": false },
    { "id": "macos", "name": "macOS universale", "os": "macos-15", "args": "--features computer-use --target universal-apple-darwin --bundles dmg", "targets": "aarch64-apple-darwin,x86_64-apple-darwin", "arm": false, "optional": false },
    { "id": "linux-arm64", "name": "Linux arm64", "os": "ubuntu-22.04-arm", "args": "--bundles deb,appimage", "targets": "", "arm": true, "optional": true },
    { "id": "windows-arm64", "name": "Windows arm64", "os": "windows-11-arm", "args": "--bundles nsis", "targets": "", "arm": true, "optional": true }
  ],
  "daemon": [
    { "id": "daemon-linux-arm64", "name": "Linux arm64", "os": "ubuntu-22.04-arm", "asset": "aiw-linux-arm64" },
    { "id": "daemon-windows-arm64", "name": "Windows arm64", "os": "windows-11-arm", "asset": "aiw-windows-arm64.exe" }
  ],
  "cuda": [
    { "id": "cuda-linux", "name": "Linux x64", "os": "ubuntu-22.04", "asset": "aiw-linux-x64-cuda12" },
    { "id": "cuda-windows", "name": "Windows x64", "os": "windows-2022", "asset": "aiw-windows-x64-cuda12.exe" }
  ]
}
JSON
}

select_targets() {
  local raw="${INPUT_TARGETS:-}"
  [ -n "${raw//[[:space:],;]/}" ] || raw="${VAR_TARGETS:-}"
  [ -n "${raw//[[:space:],;]/}" ] || raw="default"
  local sel
  sel="$(expand "$raw")"
  if [ -z "${sel// /}" ]; then
    echo "::error::nessun target di release valido in: $raw" >&2
    return 1
  fi
  local ids
  ids="$(printf '%s\n' $sel | jq -R . | jq -sc .)"
  local cat
  cat="$(catalogue)"
  local d dm cu core
  d="$(jq -c --argjson ids "$ids" '.desktop | map(select(.id as $i | $ids | index($i)))' <<<"$cat")"
  dm="$(jq -c --argjson ids "$ids" '.daemon | map(select(.id as $i | $ids | index($i)))' <<<"$cat")"
  cu="$(jq -c --argjson ids "$ids" '.cuda | map(select(.id as $i | $ids | index($i)))' <<<"$cat")"
  # The core tests are part of the default release: they run when a default target is selected.
  core=false
  case " $sel " in *" windows-x64 "* | *" macos "*) core=true ;; esac
  {
    echo "desktop=$d"
    echo "daemon=$dm"
    echo "cuda=$cu"
    echo "core=$core"
    echo "selected=${sel% }"
  } >>"${GITHUB_OUTPUT:-/dev/stdout}"
  echo "Target selezionati: ${sel% }" >&2
}

self_test() {
  local out
  run() { INPUT_TARGETS="$1" VAR_TARGETS="${2:-}" GITHUB_OUTPUT=/dev/stdout "$0" 2>/dev/null; }
  check() { # check <description> <expected output line> <actual output>
    if ! grep -qxF -- "$2" <<<"$3"; then
      echo "FAIL: $1: manca '$2'" >&2
      echo "$3" >&2
      exit 1
    fi
  }
  out="$(run "" "")"
  check "default: desktop" 'selected=macos windows-x64' "$out"
  check "default: no daemon" 'daemon=[]' "$out"
  check "default: no cuda" 'cuda=[]' "$out"
  check "default: core tests" 'core=true' "$out"
  out="$(run "linux-arm64" "")"
  check "one target: selected" 'selected=linux-arm64' "$out"
  check "one target: no core tests" 'core=false' "$out"
  check "one target: no cuda" 'cuda=[]' "$out"
  out="$(run "" "all")"
  check "variable: all" 'selected=cuda-linux cuda-windows daemon-linux-arm64 daemon-windows-arm64 linux-arm64 macos windows-arm64 windows-x64' "$out"
  out="$(run "cuda, Windows-ARM64" "all")"
  check "input wins over the variable" 'selected=cuda-linux cuda-windows windows-arm64' "$out"
  jq -e . <<<"$(grep '^desktop=' <<<"$out" | cut -d= -f2-)" >/dev/null
  out="$(run "bogus,macos" "")"
  check "unknown target ignored" 'selected=macos' "$out"
  if INPUT_TARGETS="bogus" GITHUB_OUTPUT=/dev/null "$0" 2>/dev/null; then
    echo "FAIL: una selezione vuota doveva fallire" >&2
    exit 1
  fi
  echo "select-targets: self-test ok"
}

if [ "${1:-}" = "--self-test" ]; then
  self_test
else
  select_targets
fi
