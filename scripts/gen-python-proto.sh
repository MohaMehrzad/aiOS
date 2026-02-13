#!/usr/bin/env bash
# Generate Python protobuf stubs from the aiOS .proto definitions.
#
# Usage:  ./scripts/gen-python-proto.sh
#
# Requires: grpcio-tools  (pip install grpcio-tools)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROTO_DIR="${REPO_ROOT}/agent-core/proto"
OUT_DIR="${REPO_ROOT}/agent-core/python/aios_agent/proto"

mkdir -p "${OUT_DIR}"

echo "Generating Python protobuf stubs from ${PROTO_DIR} → ${OUT_DIR}"

python3 -m grpc_tools.protoc \
    -I "${PROTO_DIR}" \
    --python_out="${OUT_DIR}" \
    --grpc_python_out="${OUT_DIR}" \
    "${PROTO_DIR}"/*.proto

# Fix imports: generated code uses `import common_pb2` but Python needs
# a package-relative import when stubs live inside a package.
for f in "${OUT_DIR}"/*_pb2_grpc.py "${OUT_DIR}"/*_pb2.py; do
    [ -f "$f" ] || continue
    # Replace bare `import xxx_pb2` with relative `from . import xxx_pb2`
    # but skip lines that are already relative or are the google imports.
    sed -i.bak -E 's/^import ([a-z_]+_pb2)/from . import \1/' "$f"
    rm -f "${f}.bak"
done

# Ensure __init__.py exists
if [ ! -f "${OUT_DIR}/__init__.py" ]; then
    echo "# Generated protobuf stubs for aiOS gRPC services." > "${OUT_DIR}/__init__.py"
fi

echo "Done. Generated stubs:"
ls -1 "${OUT_DIR}"/*.py
