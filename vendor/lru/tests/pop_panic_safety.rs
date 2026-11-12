use lru::LruCache;
use std::borrow::Borrow;
use std::num::NonZeroUsize;
use std::panic::{AssertUnwindSafe, catch_unwind};

#[derive(Hash, PartialEq, Eq)]
struct Key(u8);

impl Borrow<u8> for Key {
    fn borrow(&self) -> &u8 {
        &self.0
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        if self.0 == 1 {
            panic!("test key destructor");
        }
    }
}

#[test]
fn pop_panic_does_not_leave_freed_nodes_in_the_list() {
    let mut cache = LruCache::new(NonZeroUsize::new(2).unwrap());
    cache.put(Key(1), 1);
    cache.put(Key(2), 2);
    assert!(catch_unwind(AssertUnwindSafe(|| cache.pop(&1u8))).is_err());
    cache.put(Key(3), 3);
    cache.put(Key(4), 4);
    assert_eq!(cache.len(), 2);
    assert_eq!(cache.get(&3u8), Some(&3));
    assert_eq!(cache.get(&4u8), Some(&4));
    assert_eq!(cache.get(&2u8), None);
}
