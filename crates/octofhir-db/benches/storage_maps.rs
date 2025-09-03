#![allow(clippy::uninlined_format_args)]

use std::collections::HashMap as StdHashMap;
use dashmap::DashMap;
use papaya::HashMap as PapayaHashMap;

// Simple key/value payloads for map operations
#[inline]
fn make_key(i: u64) -> String { format!("key-{}", i) }
#[inline]
fn make_val(i: u64) -> u64 { i }

#[divan::bench]
fn std_hashmap_insert(b: divan::Bencher) {
    b.with_inputs(|| 50_000u64).bench_values(|n| {
        let mut map: StdHashMap<String, u64> = StdHashMap::with_capacity(n as usize);
        for i in 0..n { map.insert(make_key(i), make_val(i)); }
        divan::black_box(map.len())
    });
}

#[divan::bench]
fn dashmap_insert(b: divan::Bencher) {
    b.with_inputs(|| 50_000u64).bench_values(|n| {
        let map: DashMap<String, u64> = DashMap::with_capacity(n as usize);
        for i in 0..n { map.insert(make_key(i), make_val(i)); }
        divan::black_box(map.len())
    });
}

#[divan::bench]
fn papaya_insert(b: divan::Bencher) {
    b.with_inputs(|| 50_000u64).bench_values(|n| {
        let map: PapayaHashMap<String, u64> = PapayaHashMap::new();
        let guard = map.pin();
        for i in 0..n { guard.insert(make_key(i), make_val(i)); }
        divan::black_box(guard.len())
    });
}

#[divan::bench]
fn papaya_read_after_insert(b: divan::Bencher) {
    b.with_inputs(|| 50_000u64).bench_values(|n| {
        let map: PapayaHashMap<String, u64> = PapayaHashMap::new();
        {
            let guard = map.pin();
            for i in 0..n { guard.insert(make_key(i), make_val(i)); }
        }
        let guard = map.pin();
        let mut sum = 0u64;
        for i in 0..n { sum += *guard.get(&make_key(i)).unwrap(); }
        divan::black_box(sum)
    });
}

fn main() { divan::main(); }
