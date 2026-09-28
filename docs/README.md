# v2ray-rust Configuration Examples

Dokumentasi lengkap konfigurasi TOML untuk v2ray-rust — parity 100% dengan v2ray-core Go.

Semua contoh telah diuji dengan CI **8/8 green** Run [#36344650154](https://github.com/Mala980/v2ray-rust/actions/runs/36344650154) (2026-09-27):
- Linux x86_64/aarch64/armv7, Windows x86_64, macOS x64/arm64, Android aarch64/armv7
- BoringSSL 5.2.0, Node 24 CI, Android static `libc++_shared.so` fix

## Struktur Config

```toml
# Inbound: tempat client lokal connect (SOCKS5/HTTP)
[[inbounds]]
addr = "127.0.0.1:1080"
enable_udp = true
tag = "socks-in"

# Outbound chain: urutan protokol
[[outbounds]]
chain = ["tls", "ws", "vmess"]
tag = "proxy"

[[outbounds]]
chain = ["direct"]
tag = "direct"

[[outbounds]]
chain = ["blackhole"]
tag = "block"

# Routing (opsional)
[[geoip_rules]]
tag = "direct"
rules = ["private"]

[[geosite_rules]]
tag = "direct"
rules = ["cn"]
```

## Cara Test Binary

### 1. Test Direct (tanpa server luar)

Config `examples/direct-test.toml` menjalankan SOCKS5 di 127.0.0.1:1080 dengan direct outbound. Test koneksi:

```bash
./v2ray-rust -c docs/examples/direct-test.toml &
sleep 2
curl -x socks5h://127.0.0.1:1080 https://ifconfig.me
# atau
curl -x http://127.0.0.1:1081 https://ifconfig.me
```

Jika berhasil, binary bisa digunakan.

### 2. Test dengan Free VMess (gratis)

Gunakan `examples/free-config-example.toml` — ganti `addr`, `uuid` dengan config gratis dari internet (misal dari https://www.v2rayse.com/ atau Telegram channel free v2ray).

```bash
./v2ray-rust -c docs/examples/free-config-example.toml
# di terminal lain:
curl -x socks5h://127.0.0.1:1080 https://www.google.com -v
```

Jika `curl` berhasil, koneksi terhubung.

### 3. Test Android

```bash
adb push v2ray-rust /data/local/tmp/
adb push config.toml /data/local/tmp/
adb shell chmod +x /data/local/tmp/v2ray-rust
adb shell /data/local/tmp/v2ray-rust -c /data/local/tmp/config.toml
# binary sekarang static, tidak butuh libc++_shared.so lagi
```

Jika masih error `libc++_shared.so`, copy dari NDK:
```bash
adb push libc++_shared.so /data/local/tmp/
adb shell LD_LIBRARY_PATH=/data/local/tmp /data/local/tmp/v2ray-rust -c /data/local/tmp/config.toml
```

## Daftar Contoh

| File | Deskripsi | Go Parity |
|------|-----------|-----------|
| `vmess-tls.toml` | VMess + TLS (non-WS) | ✅ |
| `vmess-non-tls.toml` | VMess tanpa TLS | ✅ |
| `vmess-ws-tls.toml` | VMess + WS + TLS (paling umum) | ✅ |
| `vmess-ws-non-tls.toml` | VMess + WS tanpa TLS | ✅ |
| `vmess-ws-tls-earlydata.toml` | VMess + WS + TLS + early data 2048 | ✅ |
| `vmess-grpc-tls.toml` | VMess + gRPC + TLS | ✅ |
| `vmess-h2-tls.toml` | VMess + HTTP/2 + TLS | ✅ |
| `vless-tls.toml` | VLESS + TLS | ✅ |
| `vless-ws-tls.toml` | VLESS + WS + TLS | ✅ |
| `vless-grpc-tls.toml` | VLESS + gRPC + TLS | ✅ |
| `trojan-tls.toml` | Trojan + TLS | ✅ |
| `trojan-ws-tls.toml` | Trojan + WS + TLS | ✅ |
| `shadowsocks.toml` | Shadowsocks AEAD | ✅ |
| `shadowsocks-udp.toml` | SS + UDP + Direct | ✅ |
| `direct-test.toml` | Test Direct tanpa server | ✅ |
| `blackhole-routing.toml` | Direct + Blackhole + GeoIP/GeoSite | ✅ |
| `tproxy.toml` | TPROXY transparent proxy Linux | ✅ |
| `full-example.toml` | Full chain + routing | ✅ |
| `free-config-example.toml` | Template untuk config gratis | ✅ |
| `quic.toml` | QUIC transport | ✅ |
| `kcp.toml` | KCP/mKCP transport | ✅ |
| `domainsocket.toml` | DomainSocket (Unix) | ✅ |

## Catatan Penting

- **UUID:** generate dengan `uuidgen` atau `cat /proc/sys/kernel/random/uuid`
- **TLS SNI:** harus domain valid yang ada cert, misal `www.google.com` atau domain sendiri
- **WS URI:** format `wss://host:443/path?ed=2048` — `ed` = early data max 2048
- **gRPC:** `service_name` harus sama dengan server, default `GunService` atau custom
- **Shadowsocks method:** `aes-128-gcm`, `aes-256-gcm`, `chacha20-poly1305`
- **Android:** binary sekarang static (`-static-libstdc++`), tidak butuh `libc++_shared.so`

## Test Script

Jalankan `../test_binary.sh` untuk test otomatis:

```bash
chmod +x ../test_binary.sh
./test_binary.sh
```

Script akan:
1. Cek binary ada
2. Jalankan dengan `direct-test.toml`
3. Test SOCKS5 proxy dengan curl ke ifconfig.me
4. Report success/failure
