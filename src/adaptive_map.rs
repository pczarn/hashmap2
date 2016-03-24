// Copyright 2016 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::hash::{Hash, BuildHasher};
use std::mem::replace;
use std::ops::{Deref, DerefMut};

use adaptive_hashing::AdaptiveState;
use table::{
    RawTable,
    SafeHash
};
use internal_entry::InternalEntry;
use entry::VacantEntryState;
use entry::VacantEntryState::NeqElem;
use entry::VacantEntryState::NoElem;
use HashMap;
use search_hashed;

// Beyond this displacement, we switch to safe hashing or grow the table.
const DISPLACEMENT_THRESHOLD: usize = 128;
// When the map's load factor is below this threshold, we switch to safe hashing.
// Otherwise, we grow the table.
const LOAD_FACTOR_THRESHOLD: f32 = 0.625;

// The displacement threshold should be high enough so that even with the maximal load factor,
// it's very rarely exceeded.
// As the load approaches 90%, displacements larger than ~ 32 are much more probable.

// Avoid problems with private types in public interfaces.
pub type InternalEntryMut<'a, K: 'a, V: 'a> = InternalEntry<K, V, &'a mut RawTable<K, V>>;

// We have this trait, because specialization doesn't work for inherent impls yet.
pub trait SafeguardedSearch<K, V> {
    // Method names are changed, because inherent methods shadow trait impl
    // methods.
    fn safeguarded_search(&mut self, key: &K, hash: SafeHash) -> InternalEntryMut<K, V>;
}

impl<K, V, S> SafeguardedSearch<K, V> for HashMap<K, V, S>
    where K: Eq + Hash,
          S: BuildHasher
{
    #[inline]
    default fn safeguarded_search(&mut self, key: &K, hash: SafeHash) -> InternalEntryMut<K, V> {
        search_hashed(&mut self.table, hash, |k| k == key)
    }
}

impl<K, V> SafeguardedSearch<K, V> for HashMap<K, V, AdaptiveState>
    where K: Eq + OneshotHash
{
    #[inline]
    fn safeguarded_search(&mut self, key: &K, hash: SafeHash)
                         -> InternalEntryMut<K, V> {

        let table_capacity = self.table.capacity();
        let entry = search_hashed(DerefMapToTable(self), hash, |k| k == key);
        match entry {
            InternalEntry::Occupied { elem } => {
                // This should compile down to a no-op.
                InternalEntry::Occupied { elem: elem.convert_table() }
            }
            InternalEntry::TableIsEmpty => {
                InternalEntry::TableIsEmpty
            }
            InternalEntry::Vacant { elem, hash } => {
                safeguard_vacant_entry(elem, key, hash, table_capacity)
            }
        }
    }
}

// For correct creation of HashMap.
impl<K, V> Default for HashMap<K, V, AdaptiveState>
    where K: Eq + OneshotHash
{
    fn default() -> Self {
        HashMap::with_hasher(AdaptiveState::new_fast())
    }
}

#[inline]
fn safeguard_vacant_entry<'a, K, V>(
    elem: VacantEntryState<K, V, DerefMapToTable<'a, K, V, AdaptiveState>>,
    key: &K,
    hash: SafeHash,
    table_capacity: usize,
) -> InternalEntryMut<'a, K, V>
    where K: Eq + Hash
{
    let index = match elem {
        NeqElem(ref bucket, _) => bucket.index(),
        NoElem(ref bucket) => bucket.index(),
    };
    // Copied from FullBucket::displacement.
    let displacement = index.wrapping_sub(hash.inspect() as usize) & (table_capacity - 1);
    if displacement > DISPLACEMENT_THRESHOLD {
        // Probe sequence is too long.
        // This branch is very unlikely.
        maybe_adapt_to_safe_hashing(elem, key, hash)
    } else {
        // This should compile down to a no-op.
        match elem {
            NeqElem(bucket, ib) => {
                InternalEntry::Vacant {
                    elem: NeqElem(bucket.convert_table(), ib),
                    hash: hash,
                }
            }
            NoElem(bucket) => {
                InternalEntry::Vacant {
                    elem: NoElem(bucket.convert_table()),
                    hash: hash,
                }
            }
        }
    }
}

// Adapt to safe hashing if desirable.
#[cold]
fn maybe_adapt_to_safe_hashing<'a, K, V>(
    elem: VacantEntryState<K, V, DerefMapToTable<'a, K, V, AdaptiveState>>,
    key: &K,
    hash: SafeHash
) -> InternalEntryMut<'a, K, V>
    where K: Eq + Hash
{
    let map = match elem {
        NeqElem(bucket, _) => {
            bucket.into_table().0
        }
        NoElem(bucket) => {
            bucket.into_table().0
        }
    };
    let capacity = map.table.capacity();
    let load_factor = map.len() as f32 / capacity as f32;
    if load_factor >= LOAD_FACTOR_THRESHOLD {
        map.resize(capacity * 2);
    } else {
        // Taking this branch is as rare as proton decay. The average time between two executions of
        // this branch is 20 billion years. We assume continuous insertion on a single CPU
        // core, without intentional DoS attack.
        map.hash_builder.switch_to_safe_hashing();
        let old_table = replace(&mut map.table, RawTable::new(capacity));
        for (_, k, v) in old_table.into_iter() {
            let hash = map.make_hash(&k);
            map.insert_hashed_nocheck(hash, k, v);
        }
    }
    search_hashed(&mut map.table, hash, |k| k == key)
}

pub trait OneshotHash: Hash {}

impl OneshotHash for i8 {}
impl OneshotHash for u8 {}
impl OneshotHash for u16 {}
impl OneshotHash for i16 {}
impl OneshotHash for u32 {}
impl OneshotHash for i32 {}
impl OneshotHash for u64 {}
impl OneshotHash for i64 {}
impl OneshotHash for usize {}
impl OneshotHash for isize {}
impl OneshotHash for char {}
impl<T> OneshotHash for *const T {}
impl<T> OneshotHash for *mut T {}
impl<'a, T> OneshotHash for &'a T where T: OneshotHash {}
impl<'a, T> OneshotHash for &'a mut T where T: OneshotHash {}

struct DerefMapToTable<'a, K: 'a, V: 'a, S: 'a>(&'a mut HashMap<K, V, S>);

impl<'a, K, V, S> Deref for DerefMapToTable<'a, K, V, S> {
    type Target = RawTable<K, V>;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.0.table
    }
}

impl<'a, K, V, S> DerefMut for DerefMapToTable<'a, K, V, S> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0.table
    }
}

impl<'a, K, V, S> Into<&'a mut RawTable<K, V>> for DerefMapToTable<'a, K, V, S> {
    #[inline(always)]
    fn into(self) -> &'a mut RawTable<K, V> {
        &mut self.0.table
    }
}

#[cfg(test)]
mod test_adaptive_map {
    use HashMap;
    use super::DISPLACEMENT_THRESHOLD;

    // These values all hash to N * 2^24 + 1523546 +/- 2.
    static VALUES: &'static [u32] = &[
        513314, 2977019, 3921903, 5005242, 6124431, 7696812, 16129307, 16296222, 17425488,
        17898424, 19926075, 24768203, 25614709, 29006382, 30234341, 32377109, 34394074,
        40324616, 40892565, 43025295, 43208269, 43761687, 43883113, 45274367, 47850630,
        48320162, 48458322, 48960668, 49470322, 50545229, 51305930, 51391781, 54465806,
        54541272, 55497339, 55788640, 57113511, 58250085, 58326435, 59316149, 62059483,
        64136437, 64978683, 65076823, 66571125, 66632487, 68067917, 69921206, 70107088,
        71829636, 76189936, 78639014, 80841986, 81844602, 83028134, 85818283, 86768196,
        90374529, 91119955, 91540016, 93761675, 94583431, 95027700, 95247246, 95564585,
        95663108, 95742804, 96147866, 97538112, 101129622, 101782620, 102170444,
        104790535, 104815436, 105802703, 106364729, 106520836, 106563112, 107893429,
        112185856, 113337504, 116895916, 122566166, 123359972, 123897385, 124028529,
        125100458, 127234401, 128292718, 129767575, 132088268, 133737047, 133796663,
        135903283, 136513103, 138868673, 139106372, 141282728, 141628856, 143250884,
        143784740, 149114217, 150882858, 151116713, 152221499, 154271016, 155574791,
        156179900, 157228942, 157518087, 159572211, 161327800, 161750984, 162237441,
        164793050, 165064176, 166764350, 166847618, 167111553, 168117915, 169230761,
        170322861, 170937855, 172389295, 173619266, 177610645, 178415544, 179549865,
        185538500, 185906457, 195946437, 196591640, 196952032, 197505405, 200021193,
        201058930, 201496104, 204691301, 206144773, 207320627, 211221882, 215434456,
    ];

    #[test]
    fn test_dos_safeguard() {
        let mut map = HashMap::new();
        let mut values = VALUES.iter();
        for &value in (&mut values).take(DISPLACEMENT_THRESHOLD - 1) {
            map.insert(value, ());
        }
        assert!(!map.hash_builder.uses_safe_hashing());
        for &value in values.take(8) {
            map.insert(value, ());
        }
        assert!(map.hash_builder.uses_safe_hashing());
    }

    #[test]
    fn test_adaptive_lots_of_insertions() {
        let mut m = HashMap::new();

        // Try this a few times to make sure we never screw up the hashmap's
        // internal state.
        for _ in 0..10 {
            assert!(m.is_empty());

            for i in 1 ... 1000 {
                assert!(m.insert(i, i).is_none());

                for j in 1...i {
                    let r = m.get(&j);
                    assert_eq!(r, Some(&j));
                }

                for j in i+1...1000 {
                    let r = m.get(&j);
                    assert_eq!(r, None);
                }
            }

            for i in 1001...2000 {
                assert!(!m.contains_key(&i));
            }

            // remove forwards
            for i in 1...1000 {
                assert!(m.remove(&i).is_some());

                for j in 1...i {
                    assert!(!m.contains_key(&j));
                }

                for j in i+1...1000 {
                    assert!(m.contains_key(&j));
                }
            }

            for i in 1...1000 {
                assert!(!m.contains_key(&i));
            }

            for i in 1...1000 {
                assert!(m.insert(i, i).is_none());
            }

            // remove backwards
            for i in (1..1001).rev() {
                assert!(m.remove(&i).is_some());

                for j in i...1000 {
                    assert!(!m.contains_key(&j));
                }

                for j in 1...i-1 {
                    assert!(m.contains_key(&j));
                }
            }
        }
    }
}
