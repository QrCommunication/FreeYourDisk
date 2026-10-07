#!/usr/bin/env bash
# Convert the GitHub PKCS#12 secret to algorithms accepted by Apple Keychain.
set +x
set -euo pipefail
umask 077

if [[ $# != 1 || -z ${APPLE_CERTIFICATE:-} || -z ${APPLE_CERTIFICATE_PASSWORD:-} ]]; then
  echo 'Usage: set APPLE_CERTIFICATE and APPLE_CERTIFICATE_PASSWORD, then pass the output .p12 path.' >&2
  exit 1
fi

if [[ -n ${OPENSSL_BIN:-} ]]; then
  openssl_bin=$OPENSSL_BIN
elif [[ $(uname -s) == Darwin ]]; then
  openssl_bin="$(brew --prefix openssl@3)/bin/openssl"
else
  openssl_bin=$(command -v openssl)
fi

export APPLE_CERTIFICATE_PASSWORD
APPLE_CERTIFICATE_PASSWORD=$(printf '%s' "$APPLE_CERTIFICATE_PASSWORD" | tr -d '\r\n')
test -n "$APPLE_CERTIFICATE_PASSWORD"

temporary_dir=$(mktemp -d "${TMPDIR:-/tmp}/freeyourdisk-signing.XXXXXX")
trap 'rm -rf "$temporary_dir"' EXIT
original_p12="$temporary_dir/original.p12"
encrypted_pem="$temporary_dir/encrypted.pem"
compatible_p12="$temporary_dir/compatible.p12"

printf '%s' "$APPLE_CERTIFICATE" | "$openssl_bin" base64 -d -A > "$original_p12"
"$openssl_bin" pkcs12 -in "$original_p12" -noout \
  -passin env:APPLE_CERTIFICATE_PASSWORD
# Never write an unencrypted private key, including the intermediate export.
"$openssl_bin" pkcs12 -in "$original_p12" -out "$encrypted_pem" \
  -passin env:APPLE_CERTIFICATE_PASSWORD -passout env:APPLE_CERTIFICATE_PASSWORD
"$openssl_bin" pkcs12 -export -in "$encrypted_pem" -out "$compatible_p12" \
  -passin env:APPLE_CERTIFICATE_PASSWORD -passout env:APPLE_CERTIFICATE_PASSWORD \
  -keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES -macalg sha1
"$openssl_bin" pkcs12 -in "$compatible_p12" -noout \
  -passin env:APPLE_CERTIFICATE_PASSWORD
mv "$compatible_p12" "$1"
