pub(crate) fn murmur_hash_v3(key: &str, seed: u32) -> u32 {
    let bytes = key.as_bytes();
    let mut h1 = seed;
    let c1: u32 = 0xcc9e2d51;
    let c2: u32 = 0x1b873593;

    for chunk in bytes.chunks_exact(4) {
        let mut k1 = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        k1 = k1.wrapping_mul(c1).rotate_left(15).wrapping_mul(c2);
        h1 ^= k1;
        h1 = h1.rotate_left(13).wrapping_mul(5).wrapping_add(0xe6546b64);
    }

    let remainder = bytes.chunks_exact(4).remainder();
    let mut k1 = 0u32;
    for (index, byte) in remainder.iter().enumerate() {
        k1 ^= (*byte as u32) << (index * 8);
    }
    if !remainder.is_empty() {
        k1 = k1.wrapping_mul(c1).rotate_left(15).wrapping_mul(c2);
        h1 ^= k1;
    }

    h1 ^= bytes.len() as u32;
    h1 ^= h1 >> 16;
    h1 = h1.wrapping_mul(0x85ebca6b);
    h1 ^= h1 >> 13;
    h1 = h1.wrapping_mul(0xc2b2ae35);
    h1 ^ (h1 >> 16)
}
