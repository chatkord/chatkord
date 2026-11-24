#!/usr/bin/env bash
# A nextest setup script (client/.config/nextest.toml): the tests it covers run under Khronos' validation layer, a
# validation error ending the test, when the Vulkan loader finds the layer; without it, nothing is set and
# `tier::validated` says the test was skipped. `gpu` adds GPU-assisted validation, which sees a shader's stores.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=../protocol/scripts/find-python.sh
. "$root/protocol/scripts/find-python.sh"
[ -n "$PYTHON" ] || { echo "no Python $PYTHON_FLOOR or later on PATH" >&2; exit 1; }

# The loader is asked, as every Vulkan program asks it, so a layer it would not load is not claimed.
# shellcheck disable=SC2086  # "py -3" is two words
found="$($PYTHON -I -c '
import ctypes, sys
try:
    loader = ctypes.WinDLL("vulkan-1.dll") if sys.platform == "win32" else ctypes.CDLL("libvulkan.so.1")
except OSError:
    sys.exit()
class Layer(ctypes.Structure):
    _fields_ = [("name", ctypes.c_char * 256), ("spec", ctypes.c_uint32), ("version", ctypes.c_uint32),
                ("description", ctypes.c_char * 256)]
count = ctypes.c_uint32()
loader.vkEnumerateInstanceLayerProperties(ctypes.byref(count), None)
layers = (Layer * count.value)()
loader.vkEnumerateInstanceLayerProperties(ctypes.byref(count), layers)
print(" ".join(layer.name.decode() for layer in layers[: count.value]))
')"
case " $found " in
*" VK_LAYER_KHRONOS_validation "*) ;;
*) exit 0 ;;
esac

{
    echo "VK_INSTANCE_LAYERS=VK_LAYER_KHRONOS_validation"
    echo "VK_LAYER_REPORT_FLAGS=error"
    echo "VK_LAYER_DEBUG_ACTION=VK_DBG_LAYER_ACTION_LOG_MSG,VK_DBG_LAYER_ACTION_BREAK"
    # Not ours: a device's create info chaining a structure newer than the layer's headers (FFmpeg 9's and Mesa's,
    # against Fedora 44's layer 1.4.341).
    echo "VK_LAYER_MESSAGE_ID_FILTER=0x901f59ec"
    if [ "${1:-}" = gpu ]; then
        echo "VK_LAYER_ENABLES=VK_VALIDATION_FEATURE_ENABLE_GPU_ASSISTED_EXT"
    fi
} >> "$NEXTEST_ENV"
