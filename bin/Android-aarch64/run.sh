#!/system/bin/sh
# Wrapper for v2ray-rust Android binary with bundled libc++_shared.so
# Fixes CANNOT LINK EXECUTABLE libc++_shared.so not found
# Usage: adb push bin/Android-aarch64/* /data/local/tmp/ && adb shell chmod +x /data/local/tmp/v2ray-rust /data/local/tmp/run.sh && adb shell /data/local/tmp/run.sh -c /data/local/tmp/config.toml

DIR=$(dirname "$0")
export LD_LIBRARY_PATH="$DIR:$LD_LIBRARY_PATH"
# Also try current dir
export LD_LIBRARY_PATH=".:$LD_LIBRARY_PATH"

echo "Running v2ray-rust with LD_LIBRARY_PATH=$LD_LIBRARY_PATH"
echo "Binary: $DIR/v2ray-rust"
echo "Config: $1"

# Check if binary needs libc++_shared.so
if [ -f "$DIR/libc++_shared.so" ]; then
  echo "Found bundled libc++_shared.so in $DIR"
else
  echo "WARNING: libc++_shared.so not found in $DIR, trying system lib"
fi

exec "$DIR/v2ray-rust" "$@"
