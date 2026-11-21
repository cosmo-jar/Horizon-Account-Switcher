// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

use std::sync::atomic::{Ordering, compiler_fence};

/// RAII byte buffer that zeroes key/plaintext memory before deallocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretBytes {
    buf: Vec<u8>,
}

impl SecretBytes {
    /// Wraps raw secret bytes in an auto-wiping buffer.
    pub fn new(buf: Vec<u8>) -> Self {
        Self { buf }
    }

    /// Reads the protected bytes as a slice while alive.
    pub fn as_slice(&self) -> &[u8] {
        &self.buf
    }
}

impl Drop for SecretBytes {
    fn drop(&mut self) {
        wipe_bytes(&mut self.buf);
    }
}

/// Overwrites every byte in `bytes` using volatile writes so the compiler cannot elide wiping.
pub fn wipe_bytes(bytes: &mut [u8]) {
    for b in bytes.iter_mut() {
        // SAFETY: `b` is a valid mutable reference to initialized stack/heap memory.
        unsafe { std::ptr::write_volatile(b, 0) };
    }
    compiler_fence(Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wipe_bytes_zeroes_buffer() {
        let mut data = vec![0xAA, 0xBB, 0xCC, 0xDD];
        wipe_bytes(&mut data);
        assert_eq!(data, vec![0, 0, 0, 0]);
    }
}
