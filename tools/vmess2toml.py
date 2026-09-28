#!/usr/bin/env python3
"""
Convert vmess:// link to v2ray-rust TOML config
"""
import base64, json, sys, urllib.parse

def decode_vmess(link):
    link = link.strip()
    if link.startswith("vmess://"):
        b64 = link[8:]
        # Add padding if needed
        b64 += "=" * (-len(b64) % 4)
        data = base64.b64decode(b64).decode('utf-8')
        return json.loads(data)
    raise ValueError("Not vmess link")

def vmess_to_toml(vmess_json, tag_prefix="vmess"):
    add = vmess_json.get("add", "")
    port = vmess_json.get("port", "")
    id_ = vmess_json.get("id", "")
    net = vmess_json.get("net", "")
    path = vmess_json.get("path", "")
    host = vmess_json.get("host", "")
    sni = vmess_json.get("sni", "")
    tls = vmess_json.get("tls", "")
    scy = vmess_json.get("scy", "auto")
    ps = vmess_json.get("ps", "")
    
    # Map scy to v2ray-rust method
    if scy == "none" or scy == "zero":
        method = "none"
    elif scy == "auto":
        method = "aes-128-gcm"
    else:
        method = scy if scy in ["aes-128-gcm", "chacha20-poly1305"] else "aes-128-gcm"
    
    addr = f"{add}:{port}"
    
    toml = f"# {ps}\n"
    toml += f"# Original: vmess://... (decoded)\n"
    toml += f"# add={add} port={port} net={net} path={path} tls={tls} sni={sni} host={host}\n\n"
    
    toml += f"[[vmess]]\n"
    toml += f"addr = \"{addr}\"\n"
    toml += f"uuid = \"{id_}\"\n"
    toml += f"method = \"{method}\"\n"
    toml += f"tag = \"{tag_prefix}\"\n\n"
    
    if net == "ws":
        # Build WS URI
        scheme = "wss" if tls == "tls" else "ws"
        # Use sni or host or add as host
        ws_host = sni or host or add
        uri = f"{scheme}://{ws_host}:{port}{path}"
        # Add early data if needed
        if "ed=" not in uri:
            uri += "?ed=2048" if "?" not in path else "&ed=2048"
        toml += f"[[ws]]\n"
        toml += f"uri = \"{uri}\"\n"
        if host:
            toml += f"headers = {{ Host = \"{host}\" }}\n"
        toml += f"tag = \"ws\"\n\n"
    
    if tls == "tls":
        toml += f"[[tls]]\n"
        toml += f"sni = \"{sni or host or add}\"\n"
        toml += f"tag = \"tls\"\n\n"
    
    toml += f"[[direct]]\n"
    toml += f"tag = \"direct\"\n\n"
    
    toml += f"[[outbounds]]\n"
    chain = []
    if tls == "tls":
        chain.append("tls")
    if net == "ws":
        chain.append("ws")
    chain.append(tag_prefix)
    toml += f"chain = {chain}\n"
    toml += f"tag = \"proxy\"\n\n"
    
    toml += f"[[outbounds]]\n"
    toml += f"chain = [\"direct\"]\n"
    toml += f"tag = \"direct-out\"\n\n"
    
    toml += f"[[inbounds]]\n"
    toml += f"addr = \"127.0.0.1:1080\"\n"
    toml += f"enable_udp = true\n"
    toml += f"tag = \"socks\"\n\n"
    
    toml += f"[[inbounds]]\n"
    toml += f"addr = \"127.0.0.1:1081\"\n"
    toml += f"tag = \"http\"\n"
    
    return toml

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: vmess2toml.py vmess://...")
        sys.exit(1)
    for link in sys.argv[1:]:
        try:
            j = decode_vmess(link)
            print(json.dumps(j, indent=2))
            print("\n--- TOML ---\n")
            print(vmess_to_toml(j))
        except Exception as e:
            print(f"Error: {e}", file=sys.stderr)
