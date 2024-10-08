#!/bin/bash

set -e

CURRDIR="$(pwd)"

if [ -z "${PREFIX}" ]; then
    PREFIX="${CURRDIR}/plugins"
fi

PLUGINS_VERSION="2.4.0"

cd plugins
if [ ! -f "monitoring-plugins-${PLUGINS_VERSION}.tar.gz" ]; then

    curl -O "https://www.monitoring-plugins.org/download/monitoring-plugins-${PLUGINS_VERSION}.tar.gz"
fi
tar -xvf "monitoring-plugins-${PLUGINS_VERSION}.tar.gz"
mv "monitoring-plugins-${PLUGINS_VERSION}" monitoring-plugins

cd "${CURRDIR}" || exit 1


./scripts/fix_plugins_ioctl.sh

if [ -n "${OPENSSL_DIR}" ]; then
    OPENSSL_DIR_STRING="--with-openssl=${OPENSSL_DIR}"
else
    OPENSSL_DIR_STRING=""
fi

# shellcheck disable=SC2086
cd plugins/monitoring-plugins && ./configure \
    --prefix="${PREFIX}" \
    --without-systemd \
    --with-snmpget="$(which snmpget)" \
    --with-snmpgetnext="$(which snmpgetnext)" \
    --with-ipv6 \
    $OPENSSL_DIR_STRING \
    && make
