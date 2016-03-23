// Copyright 2016 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::hash::{BuildHasher, SipHasher, Hasher};

use sip_hash_state::SipHashState;

#[derive(Clone)]
pub struct AdaptiveState {
    inner: Option<SipHashState>
}

impl AdaptiveState {
    #[inline]
    pub fn new() -> Self {
        Default::default()
    }

    #[inline]
    pub fn new_fast() -> Self {
        AdaptiveState {
            inner: None
        }
    }

    #[inline]
    pub fn switch_to_safe_hashing(&mut self) {
        self.inner = Some(SipHashState::new());
    }

    pub fn uses_safe_hashing(&self) -> bool {
        self.inner.is_some()
    }
}

// For correct creation of HashMap.
impl Default for AdaptiveState {
    fn default() -> Self {
        let mut this = AdaptiveState::new_fast();
        this.switch_to_safe_hashing();
        this
    }
}

impl BuildHasher for AdaptiveState {
    type Hasher = AdaptiveHasher;
    #[inline]
    fn build_hasher(&self) -> AdaptiveHasher {
        AdaptiveHasher {
            safe_hasher: self.inner.as_ref().map(|state| state.build_hasher()),
            hash: 0,
        }
    }
}

pub struct AdaptiveHasher {
    safe_hasher: Option<SipHasher>,
    hash: u64,
}

/// Load a full u64 word from a byte stream, in LE order. Use
/// `copy_nonoverlapping` to let the compiler generate the most efficient way
/// to load u64 from a possibly unaligned address.
///
/// Unsafe because: unchecked indexing at 0..len
#[inline]
unsafe fn load_u64_le(buf: &[u8], len: usize) -> u64 {
    use std::ptr;
    debug_assert!(len <= buf.len());
    let mut data = 0u64;
    ptr::copy_nonoverlapping(buf.as_ptr(), &mut data as *mut _ as *mut u8, len);
    data.to_le()
}

// Primes used in XXH64's finalizer.
const PRIME_2: u64 = 14029467366897019727;
const PRIME_3: u64 = 1609587929392839161;

// Xxhash's finalizer.
fn mix(data: u64) -> u64 {
    let mut hash = data;
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(PRIME_2);
    hash ^= hash >> 29;
    hash = hash.wrapping_mul(PRIME_3);
    hash ^= hash >> 32;
    hash
}

impl Hasher for AdaptiveHasher {
    #[inline]
    fn write(&mut self, msg: &[u8]) {
        if let Some(ref mut hasher) = self.safe_hasher {
            hasher.write(msg);
        } else {
            let msg_data = unsafe {
                if msg.len() <= 8 {
                    load_u64_le(msg, msg.len())
                } else {
                    panic!()
                }
            };
            self.hash = mix(msg_data);
        }
    }

    #[inline]
    fn finish(&self) -> u64 {
        if let Some(ref hasher) = self.safe_hasher {
            hasher.finish()
        } else {
            self.hash
        }
    }
}
