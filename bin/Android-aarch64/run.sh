#!/system/bin/sh
# v2ray-rust Android binary - now truly static with rustls (pure Rust TLS)
# No longer needs libc++_shared.so
# Usage: adb push bin/Android-aarch64/v2ray-rust /data/local/tmp/ && adb shell chmod +x /data/local/tmp/v2ray-rust && adb shell /data/local/tmp/v2ray-rust -c /data/local/tmp/config.toml

DIR=$(dirname "$0")
echo "Running v2ray-rust (rustls static) from $DIR"
echo "Binary is truly static: only needs libdl.so, libc.so"
echo "No LD_LIBRARY_PATH needed anymore!"

exec "$DIR/v2ray-rust" "$@"
