#!/usr/bin/env bash
set -euo pipefail
umask 077
if [[ -e /runner/.boxology-rustup-volume-seed ]]; then
  rm -f /runner/.boxology-rustup-volume-seed
elif [[ -n "$(find /runner -mindepth 1 -maxdepth 1 -print -quit)" ]]; then
  printf '%s\n' 'runner: state volume is not fresh' >&2
  exit 75
fi
cp -a /opt/actions-runner/. /runner/
cd /runner
mkdir -p home _work/.cargo _work/.rustup _work/_temp
export HOME=/runner/home CARGO_HOME=/runner/_work/.cargo RUSTUP_HOME=/runner/_work/.rustup RUNNER_TEMP=/runner/_work/_temp

jit_config="$(cat)"
if [[ -z "$jit_config" || "$jit_config" =~ [^A-Za-z0-9+/=_-] ]]; then
  printf '%s\n' 'runner: missing or malformed JIT configuration' >&2
  exit 64
fi
export ImageOS=ubuntu24.04-arm64-colima
export ImageVersion=ubuntu-24.04-arm64-runner-2.336.0-rust-1.97.1-deny-0.20.2-rustup-volume-1
unset GITHUB_TOKEN GH_TOKEN RUNNER_TOKEN ACTIONS_RUNTIME_TOKEN \
  AWS_ACCESS_KEY_ID AWS_SECRET_ACCESS_KEY AZURE_CLIENT_SECRET

status=0
./run.sh --disableupdate --jitconfig "$jit_config" || status=$?
unset jit_config
rm -f .runner .credentials .credentials_rsaparams .env
exit "$status"
