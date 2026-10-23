//! `traverse_objects` and `prune_objects` keep the objects they have seen in a set. These tests
//! compare them with the earlier implementations, which searched a vector, on random object
//! graphs.

use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use std::collections::HashSet;

/// A small deterministic generator, so that a failing seed can be reproduced.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// A reference to one of `n` objects, or now and then to a missing one.
fn random_reference(rng: &mut Rng, n: u32) -> Object {
    Object::Reference((rng.below(n as u64 + 3) as u32 + 1, 0))
}

fn random_value(rng: &mut Rng, n: u32, depth: u32) -> Object {
    match rng.below(if depth == 0 { 3 } else { 6 }) {
        0 | 1 => random_reference(rng, n),
        2 => Object::Integer(rng.below(10) as i64),
        3 => {
            // Arrays often repeat a reference, which deletion removes one at a time.
            let len = rng.below(5);
            let mut array: Vec<Object> = (0..len).map(|_| random_value(rng, n, depth - 1)).collect();
            if let Some(first) = array.first().cloned()
                && rng.below(2) == 0
            {
                array.push(first);
            }
            Object::Array(array)
        }
        _ => Object::Dictionary(random_dictionary(rng, n, depth - 1)),
    }
}

fn random_dictionary(rng: &mut Rng, n: u32, depth: u32) -> Dictionary {
    let mut dict = Dictionary::new();
    for _ in 0..rng.below(6) {
        dict.set(format!("K{}", rng.below(8)), random_value(rng, n, depth));
    }
    dict
}

/// `n` objects that reference each other at random, and a trailer that references a few.
fn random_document(seed: u64, n: u32) -> Document {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut doc = Document::with_version("1.7");
    for number in 1..=n {
        let object = match rng.below(5) {
            0 => {
                let content = if rng.below(2) == 0 { vec![] } else { b"q Q".to_vec() };
                Object::Stream(Stream::new(random_dictionary(&mut rng, n, 2), content))
            }
            1 => random_value(&mut rng, n, 2),
            _ => Object::Dictionary(random_dictionary(&mut rng, n, 2)),
        };
        doc.objects.insert((number, 0), object);
    }
    doc.max_id = n;
    for key in ["Root", "Info", "Extra"] {
        doc.trailer.set(key, random_reference(&mut rng, n));
    }
    doc
}

/// The earlier `traverse_objects`, which looked up every reference in a vector.
fn old_traverse_objects<A: Fn(&mut Object)>(doc: &mut Document, action: A) -> Vec<ObjectId> {
    fn traverse_object<A: Fn(&mut Object)>(object: &mut Object, action: &A, refs: &mut Vec<ObjectId>) {
        action(object);
        match object {
            Object::Array(array) => array.iter_mut().for_each(|item| traverse_object(item, action, refs)),
            Object::Dictionary(dict) => dict.iter_mut().for_each(|(_, v)| traverse_object(v, action, refs)),
            Object::Stream(stream) => stream
                .dict
                .iter_mut()
                .for_each(|(_, v)| traverse_object(v, action, refs)),
            Object::Reference(id) if !refs.contains(id) => refs.push(*id),
            _ => {}
        }
    }
    let mut refs = vec![];
    for (_, v) in doc.trailer.iter_mut() {
        traverse_object(v, &action, &mut refs);
    }
    let mut index = 0;
    while index < refs.len() {
        if let Some(object) = doc.objects.get_mut(&refs[index]) {
            traverse_object(object, &action, &mut refs);
        }
        index += 1;
    }
    refs
}

#[test]
fn traverse_objects_visits_references_in_the_same_order() {
    for seed in 0..300 {
        let mut doc = random_document(seed, 40);
        let mut old = doc.clone();
        assert_eq!(
            doc.traverse_objects(|_| {}),
            old_traverse_objects(&mut old, |_| {}),
            "seed {seed}"
        );
    }
}

#[test]
fn prune_objects_removes_the_same_objects() {
    for seed in 0..300 {
        let mut doc = random_document(seed, 40);
        let reachable: HashSet<ObjectId> = old_traverse_objects(&mut doc.clone(), |_| {}).into_iter().collect();
        let expected: Vec<ObjectId> = doc
            .objects
            .keys()
            .filter(|id| !reachable.contains(id))
            .copied()
            .collect();
        assert_eq!(doc.prune_objects(), expected, "seed {seed}");
        assert!(doc.objects.keys().all(|id| reachable.contains(id)), "seed {seed}");
    }
}

#[test]
fn pruning_a_large_document_takes_linear_time() {
    // Searching a vector of the IDs seen took over half a minute for this many objects.
    let n = 200_000;
    let mut doc = Document::with_version("1.7");
    let kids: Vec<Object> = (2..n + 2).map(|number| Object::Reference((number, 0))).collect();
    doc.objects.insert((1, 0), Object::Array(kids));
    for number in 2..n + 2 {
        doc.objects.insert((number, 0), Object::Integer(number as i64));
    }
    for number in n + 2..n + 102 {
        doc.objects.insert((number, 0), Object::Integer(0));
    }
    doc.trailer.set("Root", (1, 0));
    assert_eq!(doc.traverse_objects(|_| {}).len(), n as usize + 1);
    assert_eq!(
        doc.prune_objects(),
        (n + 2..n + 102).map(|number| (number, 0)).collect::<Vec<_>>()
    );
}
