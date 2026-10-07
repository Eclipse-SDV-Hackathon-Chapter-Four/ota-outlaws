#!/bin/bash

# Change the path to where the AZ3166 is mounted on your system.
DESTINATION=${1:-/Volumes/AZ3166}
CONFIG=${2:-starter}

# Get the absolute path to the AZ3166 directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUILD_DIR="${SCRIPT_DIR}/../build/${CONFIG}"

if [ ! -d "${DESTINATION}" ]; then
    echo "[ERROR] Destination ${DESTINATION} does not exist!"
    exit 1
fi

BINARY=$(ls "${BUILD_DIR}"/app/*.bin 2>/dev/null | head -n 1)

if [ -f "${BINARY}" ]; then
    echo "[INFO] Copying ${BINARY} to ${DESTINATION}..."
    cp "${BINARY}" "${DESTINATION}"
    echo "[OK] Deployment successful!"
else
    echo "[ERROR] No binary found in ${BUILD_DIR}/app/*.bin"
    exit 1
fi
