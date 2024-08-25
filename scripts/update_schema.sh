#!/bin/bash

SCHEMA_TEMP_DIR=$(mktemp -d)

NEWFILE="$SCHEMA_TEMP_DIR/maremma.schema.json"
OLDFILE="./maremma.schema.json"
cargo run -- export-config-schema > "${NEWFILE}"

if [ ! -f "${OLDFILE}" ]; then
    echo "No old schema file found, creating a new one"
    mv "${NEWFILE}" "${OLDFILE}"
    rm -rf "${SCHEMA_TEMP_DIR}"
    exit 0
fi

DIFFSTR="$(diff "${OLDFILE}" "${NEWFILE}")"

if [ "$(diff "${OLDFILE}" "${NEWFILE}")" != "" ]; then
    echo "Schema has changed, updating the repo schema file"
    echo "$DIFFSTR"
    mv "${NEWFILE}" "${OLDFILE}"
else
    echo "Schema has not changed"
fi
rm -rf "${SCHEMA_TEMP_DIR}"