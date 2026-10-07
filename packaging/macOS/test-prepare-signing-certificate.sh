#!/usr/bin/env bash
# Exercise conversion with a disposable certificate; no real signing material.
set +x
set -euo pipefail
umask 077

openssl_bin=${OPENSSL_BIN:-$(command -v openssl)}
script_dir=$(cd "$(dirname "$0")" && pwd)
fixture_dir=$(mktemp -d "${TMPDIR:-/tmp}/freeyourdisk-signing-test.XXXXXX")
trap 'rm -rf "$fixture_dir"' EXIT

export APPLE_CERTIFICATE_PASSWORD='disposable-fixture-password'
"$openssl_bin" req -x509 -newkey rsa:2048 -days 1 -subj '/CN=Disposable CI fixture' \
  -keyout "$fixture_dir/key.pem" -out "$fixture_dir/cert.pem" \
  -passout env:APPLE_CERTIFICATE_PASSWORD >/dev/null 2>&1
"$openssl_bin" pkcs12 -export -inkey "$fixture_dir/key.pem" -in "$fixture_dir/cert.pem" \
  -out "$fixture_dir/modern.p12" \
  -passin env:APPLE_CERTIFICATE_PASSWORD -passout env:APPLE_CERTIFICATE_PASSWORD
export APPLE_CERTIFICATE
APPLE_CERTIFICATE=$("$openssl_bin" base64 -A -in "$fixture_dir/modern.p12")
# Secrets pasted from password files may retain terminal CR/LF characters.
APPLE_CERTIFICATE_PASSWORD+=$'\r\n'
OPENSSL_BIN="$openssl_bin" bash "$script_dir/prepare-signing-certificate.sh" "$fixture_dir/apple.p12"
APPLE_CERTIFICATE_PASSWORD='disposable-fixture-password'
"$openssl_bin" pkcs12 -in "$fixture_dir/apple.p12" -noout -info \
  -passin env:APPLE_CERTIFICATE_PASSWORD 2> "$fixture_dir/algorithms.txt"
grep -q 'MAC: sha1' "$fixture_dir/algorithms.txt"
test "$(grep -c 'pbeWithSHA1And3-KeyTripleDES-CBC' "$fixture_dir/algorithms.txt")" -eq 2
"$openssl_bin" pkcs12 -in "$fixture_dir/apple.p12" -clcerts -nokeys \
  -passin env:APPLE_CERTIFICATE_PASSWORD -out "$fixture_dir/converted-cert.pem"
test "$("$openssl_bin" x509 -in "$fixture_dir/cert.pem" -fingerprint -sha256 -noout)" = \
  "$("$openssl_bin" x509 -in "$fixture_dir/converted-cert.pem" -fingerprint -sha256 -noout)"

# A bad secret must fail before replacing a previously valid output.
APPLE_CERTIFICATE_PASSWORD='wrong-fixture-password'
if OPENSSL_BIN="$openssl_bin" bash "$script_dir/prepare-signing-certificate.sh" "$fixture_dir/apple.p12" >/dev/null 2>&1; then
  echo 'An invalid password was accepted.' >&2
  exit 1
fi
APPLE_CERTIFICATE_PASSWORD='disposable-fixture-password'
"$openssl_bin" pkcs12 -in "$fixture_dir/apple.p12" -noout -passin env:APPLE_CERTIFICATE_PASSWORD
echo 'PKCS#12 conversion, CR/LF normalization, certificate identity and failure preservation passed.'
