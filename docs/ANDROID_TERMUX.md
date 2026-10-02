# Android & Termux Usage - Fix libc++_shared.so & __gxx_personality_v0

## Latest Working Binary
- **CI:** [#36427047899](https://github.com/Mala980/v2ray-rust/actions/runs/36427047899) 8/8 green
- **Binary:** `bin/Android-aarch64/v2ray-rust` (7.2M) + `libc++_shared.so` (1.8M) + `run.sh`
- **Status:** Needs `libc++_shared.so` (provides `__gxx_personality_v0`), bundled as fallback

## Why needs libc++_shared.so?
`boring-sys` 5.2.0 hardcodes `CMAKE_ANDROID_STL_TYPE=c++_shared` in `build/main.rs:305`, forcing link to `libc++_shared.so`. This lib provides C++ exception handling symbol `__gxx_personality_v0`.

Attempt to `patchelf --remove-needed libc++_shared.so` removes dependency but breaks:
```
CANNOT LINK EXECUTABLE: cannot locate symbol "__gxx_personality_v0"
```
Because `__gxx_personality_v0` is undefined and was provided by `libc++_shared.so`.

**Correct fix:** Keep `libc++_shared.so` bundled and set `LD_LIBRARY_PATH`.

## Termux Fix

### Error 1: libc++_shared.so not found
```
CANNOT LINK EXECUTABLE "./v2ray-rust": library "libc++_shared.so" not found
```

### Error 2: __gxx_personality_v0 not found (after patchelf)
```
CANNOT LINK EXECUTABLE "./v2ray-rust": cannot locate symbol "__gxx_personality_v0" referenced by "/data/data/com.termux/files/home/v2ray-rust"
```

Both fixed by bundling `libc++_shared.so` and setting `LD_LIBRARY_PATH`.

### Solution for Termux

```bash
# Download via codeload (works in sandbox, blob blocked)
curl -L -o repo.tar.gz https://codeload.github.com/Mala980/v2ray-rust/tar.gz/arena/01a0dc70-v2ray-rust
tar -xzf repo.tar.gz --strip-components=1
ls -lh bin/Android-aarch64/

# Copy both files to Termux home
cp bin/Android-aarch64/v2ray-rust $HOME/
cp bin/Android-aarch64/libc++_shared.so $HOME/
cp bin/Android-aarch64/run.sh $HOME/
cp docs/examples/free-asia1-ntls.toml $HOME/config.toml

chmod +x $HOME/v2ray-rust $HOME/run.sh

# Option 1: Use wrapper script (sets LD_LIBRARY_PATH)
$HOME/run.sh -c $HOME/config.toml

# Option 2: Manual LD_LIBRARY_PATH
export LD_LIBRARY_PATH=$HOME:$LD_LIBRARY_PATH
$HOME/v2ray-rust -c $HOME/config.toml

# Option 3: Copy lib to Termux lib dir (permanent)
cp $HOME/libc++_shared.so $PREFIX/lib/
# Then binary works without LD_LIBRARY_PATH
$HOME/v2ray-rust -c $HOME/config.toml
```

### Verify binary dependencies
```bash
readelf -d $HOME/v2ray-rust | grep NEEDED
# Should show: libc++_shared.so, libdl.so, libc.so

# Check __gxx_personality_v0 is undefined but provided by libc++_shared.so
nm -D $HOME/v2ray-rust | grep __gxx_personality_v0
# U __gxx_personality_v0 (undefined, will be resolved by libc++_shared.so)

# If you used patchelf --remove-needed, you'll get:
# CANNOT LINK EXECUTABLE: cannot locate symbol __gxx_personality_v0
# So do NOT use patchelf, keep libc++_shared.so
```

## True Static Future
To produce truly static binary without `libc++_shared.so`:
- Patch `boring-sys` to use `c++_static` AND link `libunwind`, `libc++abi` statically with proper flags
- Or switch TLS from `boring` to `rustls` (no C++ dependency)
- Current workaround (bundled lib) works for Termux and Android shell

## Free Configs Test
```bash
# NTLS (port 80)
./v2ray-rust -c docs/examples/free-asia1-ntls.toml
# SOCKS5 127.0.0.1:1080, HTTP 127.0.0.1:1081

# In another Termux session:
curl -x socks5h://127.0.0.1:1080 https://ifconfig.me
# Should return IP of asia1.fufo.org egress

# TLS (port 443, SNI ruangguru.com)
./v2ray-rust -c docs/examples/free-asia1-tls.toml
```
# Truly static Android with rustls
