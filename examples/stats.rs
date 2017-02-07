extern crate hashmap2;
extern crate rand;

use hashmap2::HashMap;
use rand::Rng;

fn main() {
    let mut map: HashMap<i32, ()> = HashMap::new();
    assert_eq!(map.len(), 0);
    let mut rng = rand::weak_rng();
    let mut iter = rng.gen_iter();
    let len = 2 << 20;
    let usable_cap = (len as f32 * 0.833) as usize;
    let mut stats = vec![];
    for _ in 0..10000 {
        while map.len() < usable_cap {
            map.insert(iter.next().unwrap(), ());
        }
        map.stats(&mut stats);
        map.clear();
    }
    for (i, (displacement, forward_shift)) in stats.into_iter().enumerate() {
        println!("{}: {}\t{}", i, displacement, forward_shift);
    }
    println!("map len={:?} capacity={:?}", map.len(), map.capacity());
}
