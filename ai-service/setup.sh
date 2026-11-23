#!/usr/bin/env bash
# ==============================================================================
# AI Service Environment Setup Script
# ==============================================================================
# Creates dedicated Python virtual environment and installs deep learning
# models and computer vision pipelines (YOLOv8, InsightFace, Apple Vision OCR).
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VENV_DIR="${SCRIPT_DIR}/.venv"

echo "=== Media Extractor AI Sidecar Setup ==="
echo "Working directory: ${SCRIPT_DIR}"

# 1. Select optimal Python interpreter (prefer 3.11 or 3.10 for full binary wheel compatibility)
PYTHON_BIN=""
for candidate in python3.11 python3.12 python3.10 python3; do
    if command -v "$candidate" >/dev/null 2>&1; then
        # Check python version is <= 3.12 if possible (avoid 3.14 where binary wheels may be missing)
        ver=$("$candidate" -c "import sys; print(f'{sys.version_info.major}.{sys.version_info.minor}')")
        if [[ "$ver" == "3.11" || "$ver" == "3.12" || "$ver" == "3.10" ]]; then
            PYTHON_BIN="$candidate"
            break
        elif [[ -z "$PYTHON_BIN" ]]; then
            PYTHON_BIN="$candidate"
        fi
    fi
done

if [[ -z "$PYTHON_BIN" ]]; then
    echo "ERROR: python3 could not be found. Please install Python 3.11 or 3.12." >&2
    exit 1
fi

echo "Using Python: $(command -v "$PYTHON_BIN") ($("$PYTHON_BIN" --version))"

# 2. Create virtual environment if missing
if [[ ! -f "${VENV_DIR}/bin/python3" ]]; then
    echo "Creating virtual environment at ${VENV_DIR}..."
    "$PYTHON_BIN" -m venv "${VENV_DIR}"
else
    echo "Virtual environment already exists at ${VENV_DIR}"
fi

# 3. Upgrade pip and install requirements
echo "Installing/updating dependencies from requirements.txt..."
"${VENV_DIR}/bin/pip" install --upgrade pip
"${VENV_DIR}/bin/pip" install -r "${SCRIPT_DIR}/requirements.txt"

echo "=== AI Service Setup Complete ==="
echo "Virtual environment ready at: ${VENV_DIR}"
