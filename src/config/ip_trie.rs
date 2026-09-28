fn normalize6(ip: u128, prefix: u8) -> u128 {
    if prefix == 0 {
        0
    } else {
        ip >> (128 - prefix) << (128 - prefix)
    }
}
fn normalize(ip: u32, prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else {
        ip >> (32 - prefix) << (32 - prefix)
    }
}

trait TrieNode {
    fn nullptr() -> Self;
}

impl TrieNode for u32 {
    fn nullptr() -> Self {
        u32::MAX
    }
}
impl TrieNode for u128 {
    fn nullptr() -> Self {
        u128::MAX
    }
}
macro_rules! impl_trie {
    ($trie_name:tt, $ip_type:tt) => {
        struct $trie_name {
            left: Vec<$ip_type>,
            right: Vec<$ip_type>,
            value: Vec<u32>,
            size: usize,
        }

        impl $trie_name {
            pub fn new() -> Self {
                Self {
                    left: vec![$ip_type::nullptr()],
                    right: vec![$ip_type::nullptr()],
                    value: vec![u32::MAX],
                    size: 1,
                }
            }
            pub fn put(&mut self, key: $ip_type, prefix: u8, value: u32) {
                if prefix == 0 {
                    self.value[0] = value;
                    return;
                }
                let total_bits = std::mem::size_of::<$ip_type>() * 8;
                let mut node = 0usize;
                let mut depth = 0u8;

                // Traverse existing nodes as far as possible within prefix
                while depth < prefix {
                    let bit = (total_bits - 1 - depth as usize) as usize;
                    // Actually we need to check current bit of key at position bit
                    // Use shift to get bit
                    let key_bit = (key >> bit) & 1 as $ip_type;
                    let next = if key_bit != 0 as $ip_type {
                        self.right[node]
                    } else {
                        self.left[node]
                    };
                    if next == $ip_type::nullptr() {
                        break;
                    }
                    node = next as usize;
                    depth += 1;
                }

                if depth == prefix {
                    // Found existing node for this prefix
                    self.value[node] = value;
                    return;
                }

                // Create remaining nodes
                while depth < prefix {
                    let bit = (total_bits - 1 - depth as usize) as usize;
                    let key_bit = (key >> bit) & 1 as $ip_type;
                    let next_idx = self.size as $ip_type;
                    self.value.push(u32::MAX);
                    self.left.push($ip_type::nullptr());
                    self.right.push($ip_type::nullptr());
                    if key_bit != 0 as $ip_type {
                        self.right[node] = next_idx;
                    } else {
                        self.left[node] = next_idx;
                    }
                    node = next_idx as usize;
                    self.size += 1;
                    depth += 1;
                }
                self.value[node] = value;
            }

            pub fn get(&self, key: $ip_type) -> Option<u32> {
                let total_bits = std::mem::size_of::<$ip_type>() * 8;
                let mut bit = total_bits - 1;
                let mut value = u32::MAX;
                let mut node = 0usize;
                loop {
                    if self.value[node] != u32::MAX {
                        value = self.value[node];
                    }
                    if bit >= total_bits {
                        break;
                    }
                    // Check if node has children, if both nullptr we can still have value
                    let key_bit = (key >> bit) & 1 as $ip_type;
                    let next = if key_bit != 0 as $ip_type {
                        self.right[node]
                    } else {
                        self.left[node]
                    };
                    if next == $ip_type::nullptr() {
                        break;
                    }
                    node = next as usize;
                    if bit == 0 {
                        // Last bit, check value at child then break
                        if self.value[node] != u32::MAX {
                            value = self.value[node];
                        }
                        break;
                    }
                    bit -= 1;
                }
                if value == u32::MAX {
                    None
                } else {
                    Some(value)
                }
            }
        }
    };
}

impl_trie!(PatriciaTrie4, u32);
impl_trie!(PatriciaTrie6, u128);

pub struct GeoIPMatcher {
    trie4: PatriciaTrie4,
    trie6: PatriciaTrie6,
    outbound: Vec<String>,
}

impl Default for GeoIPMatcher {
    fn default() -> Self {
        GeoIPMatcher::new()
    }
}

impl GeoIPMatcher {
    pub fn match4(&self, ip: u32) -> &str {
        return if let Some(c) = self.trie4.get(ip) {
            self.outbound[c as usize].as_str()
        } else {
            ""
        };
    }

    pub fn match6(&self, ip: u128) -> &str {
        return if let Some(c) = self.trie6.get(ip) {
            self.outbound[c as usize].as_str()
        } else {
            ""
        };
    }

    fn get_outbound_pos(&mut self, outbound: String) -> usize {
        return if let Some(p) = self.outbound.iter().position(|x| x == &outbound) {
            p
        } else {
            let len = self.outbound.len();
            self.outbound.push(outbound);
            len
        };
    }

    pub fn put_v6(&mut self, ip6: u128, prefix: u8, outbound: String) {
        let pos = self.get_outbound_pos(outbound);
        let ip6 = normalize6(ip6, prefix);
        self.trie6.put(ip6, prefix, pos as u32);
    }
    pub fn put_v4(&mut self, ip4: u32, prefix: u8, outbound: String) {
        let pos = self.get_outbound_pos(outbound);
        let ip4 = normalize(ip4, prefix);
        self.trie4.put(ip4, prefix, pos as u32);
    }

    pub fn build(&mut self) {}

    pub fn new() -> GeoIPMatcher {
        GeoIPMatcher {
            trie4: PatriciaTrie4::new(),
            trie6: PatriciaTrie6::new(),
            outbound: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::ip_trie::PatriciaTrie4;

    #[test]
    fn test4() {
        let mut trie = PatriciaTrie4::new();
        let ip1 = u32::from_be_bytes([10, 0, 0, 0]);
        let ip2 = u32::from_be_bytes([10, 0, 3, 0]);
        trie.put(ip1, 8, 69);
        trie.put(ip1, 24, 42);
        trie.put(ip2, 24, 123);

        let ip3 = u32::from_be_bytes([10, 32, 32, 32]);
        assert_eq!(trie.get(ip3).unwrap(), 69);
        let ip4 = u32::from_be_bytes([10, 0, 0, 32]);
        assert_eq!(trie.get(ip4).unwrap(), 42);
        let ip5 = u32::from_be_bytes([10, 0, 3, 5]);
        assert_eq!(trie.get(ip5).unwrap(), 123);
    }

    #[test]
    fn test_longest_prefix() {
        let mut trie = PatriciaTrie4::new();
        // 0.0.0.0/0 -> 1
        trie.put(0, 0, 1);
        // 10.0.0.0/8 -> 2
        let ip10 = u32::from_be_bytes([10, 0, 0, 0]);
        trie.put(ip10, 8, 2);
        // 10.0.0.0/24 -> 3
        trie.put(ip10, 24, 3);
        // 192.168.0.0/16 -> 4
        let ip192 = u32::from_be_bytes([192, 168, 0, 0]);
        trie.put(ip192, 16, 4);

        assert_eq!(trie.get(u32::from_be_bytes([10, 0, 0, 5])).unwrap(), 3);
        assert_eq!(trie.get(u32::from_be_bytes([10, 1, 2, 3])).unwrap(), 2);
        assert_eq!(trie.get(u32::from_be_bytes([192, 168, 1, 1])).unwrap(), 4);
        assert_eq!(trie.get(u32::from_be_bytes([8, 8, 8, 8])).unwrap(), 1);
    }

    #[test]
    fn test_ipv6() {
        use super::PatriciaTrie6;
        let mut trie = PatriciaTrie6::new();
        // ::/0 -> 1
        trie.put(0, 0, 1);
        // 2001:db8::/32 -> 2
        let ip = 0x20010db8000000000000000000000000u128;
        trie.put(ip, 32, 2);
        assert_eq!(trie.get(0x20010db8000000000000000000000001u128).unwrap(), 2);
        assert_eq!(trie.get(0x20010db9000000000000000000000001u128).unwrap(), 1);
    }
}
