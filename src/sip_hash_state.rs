// Copyright 2016 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::hash::{BuildHasher, SipHasher};
use rand::{self, Rng};

/// `SipHashState` is a random state for `HashMap` types.
///
/// A particular instance `SipHashState` will create the same instances of
/// `Hasher`, but the hashers created by two different `SipHashState`
/// instances are unlikely to produce the same result for the same values.
#[derive(Clone)]
pub struct SipHashState {
    k0: u64,
    k1: u64,
}

impl SipHashState {
    /// Constructs a new `SipHashState` that is initialized with random keys.
    #[inline]
    #[allow(deprecated)] // rand
    pub fn new() -> SipHashState {
        let mut r = rand::thread_rng();
        SipHashState { k0: r.gen(), k1: r.gen() }
    }
}

impl BuildHasher for SipHashState {
    type Hasher = SipHasher;
    #[inline]
    fn build_hasher(&self) -> SipHasher {
        SipHasher::new_with_keys(self.k0, self.k1)
    }
}

impl Default for SipHashState {
    #[inline]
    fn default() -> SipHashState {
        SipHashState::new()
    }
}
