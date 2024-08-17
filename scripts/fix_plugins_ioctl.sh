#!/bin/bash

if [ "$(uname -s)" != "Darwin" ]; then
    exit 0
fi

TARGET_FILE="plugins/monitoring-plugins/plugins-root/check_icmp.c"

if [ ! -f "${TARGET_FILE}" ]; then
    echo "Can't find the check_icmp.c file!"
    exit 1
fi

if [ "$(grep -c ioctl "${TARGET_FILE}")" -eq 0 ]; then
    echo "The ioctl issue is already fixed!"
    exit 0
fi

sed -ibak -E 's/if\(ioctl.*/if(false)/g' "${TARGET_FILE}"
if [ "$(grep -c ioctl "${TARGET_FILE}")" -ne 0 ]; then
    echo "Failed to fix the ioctl issue!"
    exit 1
fi

echo "Done!"