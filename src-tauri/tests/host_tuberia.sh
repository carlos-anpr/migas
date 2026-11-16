#!/usr/bin/env bash
# Prueba del host de mensajería nativa por tubería: un marco u32 LE + JSON y su respuesta.
set -euo pipefail

cd "$(dirname "$0")/.."
BIN="${1:-target/debug/migas}"
BASE="$(mktemp -d)"
trap 'rm -rf "$BASE"' EXIT

if [[ ! -x "$BIN" ]]; then
  echo "No existe el binario $BIN (ejecuta cargo build)" >&2
  exit 1
fi

mkdir -p "$BASE/home/.config" "$BASE/datos"

python3 - "$BIN" "$BASE" <<'PY'
import json
import os
import struct
import subprocess
import sys

binario, base = sys.argv[1], sys.argv[2]
mensaje = {
    "v": 1,
    "tipo": "capturar",
    "url": "https://empresa.atlassian.net/browse/PROJ-1",
    "titulo": "Prueba de tubería",
    "seleccion": "texto seleccionado",
}
carga = json.dumps(mensaje, ensure_ascii=False).encode("utf-8")
marco = struct.pack("<I", len(carga)) + carga
entorno = dict(
    os.environ,
    HOME=os.path.join(base, "home"),
    XDG_CONFIG_HOME=os.path.join(base, "home", ".config"),
    MIGAS_DATA_DIR=os.path.join(base, "datos"),
)
proceso = subprocess.run(
    [binario, "chrome-extension://lnmnebdbkffhodbjackjnkoegfdgcdle/"],
    input=marco,
    capture_output=True,
    env=entorno,
    timeout=15,
)
if proceso.returncode != 0:
    sys.stderr.write(proceso.stderr.decode("utf-8", "replace"))
    sys.exit(f"El host salió con {proceso.returncode}")

salida = proceso.stdout
if len(salida) < 4:
    sys.exit("Respuesta demasiado corta")
largo = struct.unpack("<I", salida[:4])[0]
respuesta = json.loads(salida[4 : 4 + largo])
print(json.dumps(respuesta, ensure_ascii=False, indent=2))
sys.exit(0 if respuesta.get("ok") else 1)
PY
