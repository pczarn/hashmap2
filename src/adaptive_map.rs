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
use HashMap;
use search_hashed;

// Beyond this displacement, we switch to safe hashing or grow the table.
const DISPLACEMENT_THRESHOLD: usize = 128;
const FORWARD_SHIFT_THRESHOLD: usize = 512;
// When the map's load factor is below this threshold, we switch to safe hashing.
// Otherwise, we grow the table.
// const LOAD_FACTOR_THRESHOLD: f32 = 0.625;
const LOAD_FACTOR_THRESHOLD: f32 = 0.2;

// The displacement threshold should be high enough so that even with the maximal load factor,
// it's very rarely exceeded.
// As the load approaches 90%, displacements larger than ~ 20 are much more probable.
// On the other hand, the threshold should be low enough so that the same number of hashes
// easily fits in the cache and takes a reasonable time to iterate through.

// The load factor threshold should be relatively low, but high enough so that its half is not very
// low (~ 20%). We choose 62.5%, because it's a simple fraction (5/8), and its half is 31.25%.
// (When a map is grown, the load factor is halved.)

// At a load factor of α, the odds of finding the target bucket after exactly n
// unsuccesful probes[1] are
//
// Pr_α{displacement = n} =
// (1 - α) / α * ∑_{k≥1} e^(-kα) * (kα)^(k+n) / (k + n)! * (1 - kα / (k + n + 1))
//
// We use this formula to find the probability of loading half of a cache line, as well as
// the probability of triggering the DoS safeguard with an insertion:
//
// Pr_0.625{displacement > 3} = 0.036
// Pr_0.625{displacement > 128} = 2.284 * 10^-49

// Pr_0.909{displacement > 3} = 0.487
// Pr_0.909{displacement > 128} = 1.601 * 10^-11
//
// 1. Alfredo Viola (2005). Distributional analysis of Robin Hood linear probing
//    hashing with buckets.

// TODO: add one-shot hashing for String, str, arrays and other types.
// TODO: consider adding a limit for the number of fully equal hashes in a probe sequence.
// Fully equal hashes cause key comparison, which might be a problem for large string keys.

// Avoid problems with private types in public interfaces.
pub type InternalEntryMut<'a, K: 'a, V: 'a> = InternalEntry<K, V, &'a mut RawTable<K, V>>;

pub trait OneshotHash: Hash {}

// We have this trait, because specialization doesn't work for inherent impls yet.
pub trait SafeguardedSearch<K, V> {
    // Method names are changed, because inherent methods shadow trait impl
    // methods.
    fn safeguarded_search(&mut self, key: &K, hash: SafeHash) -> InternalEntryMut<K, V>;
}

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

#[inline]
fn safeguard_insertion(bucket: &mut FullBucketMut<K, V>) {
    if bucket.displacement() > DISPLACEMENT_THRESHOLD {
        self.table.set_flag(true);
        // let map = bucket.into_table().0;
        // reduce_displacement(map);
        // let hash = map.make_hash(key);
        // match search_hashed(DerefMapToTable(map), hash, |k| k == key) {
        //     InternalEntry::Occupied { elem } => {
        //         elem.convert_table()
        //     }
        //     _ => {
        //         unreachable!()
        //     }
        // }
        // reduce_displacement_and_search(bucket)
    }
    bucket
}

#[inline]
fn safeguard_forward_shifted(bucket: FullBucket<FullBucket<K, V, &mut RawTable<K, V>>>) -> FullBucket<K, V, &mut RawTable<K, V>> {
    let end_index = bucket.index();
    let bucket = bucket.into_table();
    let start_index = bucket.index();
    if end_index - start_index > FORWARD_SHIFT_THRESHOLD {
        self.table.set_flag(true);
        // let (hash, key, value) = bucket.take();
        // let map = bucket.into_table();
        // reduce_displacement(map);
        // reduce_displacement_and_search(bucket
    }
    bucket
}

impl<K, V, S> SafeguardedSearch<K, V> for HashMap<K, V, S>
    where K: Eq + Hash,
          S: BuildHasher
{
    #[inline]
    default fn safeguarded_search(key: &K, hash: SafeHash) -> InternalEntryMut<K, V> {
        search_hashed(&mut self.table, hash, |k| k == key)
    }
    #[inline]
    default fn safeguard_insertion(bucket: FullBucketMut<>) {
        search_hashed(&mut self.table, hash, |k| k == key)
    }
    #[inline]
    default fn safeguard_forward_shifted(bucket: EmptyBucket<FullBucket<>>) -> InternalEntryMut<K, V> {
        // bucket.into_table().into_mut_refs().1;
        true
    }
}

impl<K, V> SafeguardedSearch<K, V> for HashMap<K, V, AdaptiveState>
    where K: Eq + OneshotHash
{
    #[inline]
    fn safeguarded_search(&mut self, key: &K, hash: SafeHash)
                         -> InternalEntryMut<K, V> {

        let mut entry = search_hashed(DerefMapToTable(self), hash, |k| k == key);
        if let InternalEntry::Vacant { elem, hash } = entry {
            entry = safeguard_vacant_entry(elem, hash, key)
        }
        entry.convert_table()
    }

    #[cold]
    fn reduce_displacement(&mut self) {
        if self.table.size() as f32 / self.table.capacity() >= LOAD_FACTOR_THRESHOLD {
            let new_capacity = max(min_cap.next_power_of_two(), INITIAL_CAPACITY);
            self.resize(self.table.capacity() * 2);
        } else {
            // Taking this branch is extremely rare, assuming no intentional DoS attack.
            self.hash_builder.switch_to_safe_hashing();
            rebuild_table(self);
        }
    }
}

#[inline]
fn safeguard_vacant_entry<'a, K, V>(
    elem: VacantEntryState<K, V, DerefMapToTable<'a, K, V, AdaptiveState>>,
    hash: SafeHash,
    key: &K,
) -> InternalEntry<K, V, DerefMapToTable<'a, K, V, AdaptiveState>>
    where K: Eq + Hash
{
    // Check displacement.
    if elem.displacement(hash) > DISPLACEMENT_THRESHOLD {
        // Probe sequence is too long. We must reduce its length.
        // This branch is very unlikely.
        let map = elem.into_table().0;
        reduce_displacement(map);
        let hash = map.make_hash(key);
        search_hashed(DerefMapToTable(map), hash, |k| k == key)
    } else {
        // This should compile down to a simple copy.
        InternalEntry::Vacant {
            elem: elem,
            hash: hash,
        }
    }
}

#[cold]
fn reduce_displacement_and_search<'a, K, V>() -> FullBucket<> {

}

// Adapt to safe hashing, if desirable.
#[cold]
fn reduce_displacement<'a, K, V>(map: &'a mut HashMap<K, V, AdaptiveState>)
    where K: Eq + Hash
{
    let table_capacity = map.table.capacity();
    let load_factor = map.len() as f32 / table_capacity as f32;
    if load_factor >= LOAD_FACTOR_THRESHOLD {
        map.resize(table_capacity * 2);
    } else {
        // Taking this branch is extremely rare -- as rare as proton decay. That's assuming
        // continuous insertion on a single CPU core, without intentional DoS attack.
        map.hash_builder.switch_to_safe_hashing();
        rebuild_table(map);
    }
}

fn rebuild_table<K, V>(map: &mut HashMap<K, V, AdaptiveState>)
    where K: Eq + Hash
{
    let table_capacity = map.table.capacity();
    let old_table = replace(&mut map.table, RawTable::new(table_capacity));
    for (_, k, v) in old_table.into_iter() {
        let hash = map.make_hash(&k);
        map.insert_hashed_nocheck(hash, k, v);
    }
}

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

    // Regression test
    #[test]
    fn test_safeguarded_insertion() {
        let mut map = HashMap::new();
        let values = VALUES.iter().enumerate();
        for (i, &value) in values.clone() {
            map.insert(value, i);
        }
        for (i, &value) in values {
            assert_eq!(map[&value], i);
        }
    }
}
