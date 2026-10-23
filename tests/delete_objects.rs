//! `delete_pages` and `delete_zero_length_streams` delete all their objects with one traversal.
//! These tests compare them with deleting one object after another, as they did before, on
//! random object graphs.

use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};
use std::collections::BTreeSet;

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

/// The earlier `delete_object`, which traversed the document once per call.
fn old_delete_object(doc: &mut Document, id: ObjectId) -> Option<Object> {
    let action = |object: &mut Object| match object {
        Object::Array(array) => {
            if let Some(index) = array.iter().position(|item| *item == Object::Reference(id)) {
                array.remove(index);
            }
        }
        Object::Dictionary(dict) => {
            let keys: Vec<Vec<u8>> = dict
                .iter()
                .filter(|(_, item)| **item == Object::Reference(id))
                .map(|(k, _)| k.clone())
                .collect();
            for key in keys {
                dict.remove(&key);
            }
        }
        _ => {}
    };
    old_traverse_objects(doc, action);
    doc.objects.remove(&id)
}

/// The earlier `delete_pages`.
fn old_delete_pages(doc: &mut Document, page_numbers: &[u32]) {
    let pages = doc.get_pages();
    for page_number in page_numbers {
        if let Some(page) = pages
            .get(page_number)
            .and_then(|page_id| old_delete_object(doc, *page_id))
        {
            let mut page_tree_ref = page
                .as_dict()
                .and_then(|dict| dict.get(b"Parent"))
                .and_then(Object::as_reference);
            while let Ok(page_tree_id) = page_tree_ref {
                if let Some(page_tree) = doc.objects.get_mut(&page_tree_id).and_then(|pt| pt.as_dict_mut().ok()) {
                    if let Ok(count) = page_tree.get(b"Count").and_then(Object::as_i64) {
                        page_tree.set("Count", count - 1);
                    }
                    page_tree_ref = page_tree.get(b"Parent").and_then(Object::as_reference);
                } else {
                    break;
                }
            }
        }
    }
}

/// A text form of an object that keeps dictionary order, which `PartialEq` ignores.
fn canonical(object: &Object) -> String {
    match object {
        Object::Array(array) => format!("[{}]", array.iter().map(canonical).collect::<Vec<_>>().join(" ")),
        Object::Dictionary(dict) => canonical_dictionary(dict),
        Object::Stream(stream) => format!("{} stream {:?}", canonical_dictionary(&stream.dict), stream.content),
        other => format!("{other:?}"),
    }
}

fn canonical_dictionary(dict: &Dictionary) -> String {
    let entries: Vec<String> = dict
        .iter()
        .map(|(k, v)| format!("/{} {}", String::from_utf8_lossy(k), canonical(v)))
        .collect();
    format!("<<{}>>", entries.join(" "))
}

/// The trailer and every object that the trailer still reaches, which deletion must leave as the
/// earlier implementation did, and the numbers of all remaining objects.
fn reachable_state(doc: &Document) -> (String, Vec<(ObjectId, String)>, BTreeSet<ObjectId>) {
    let mut probe = doc.clone();
    let reachable: BTreeSet<ObjectId> = probe.traverse_objects(|_| {}).into_iter().collect();
    let objects = reachable
        .iter()
        .filter_map(|id| doc.objects.get(id).map(|object| (*id, canonical(object))))
        .collect();
    (
        canonical_dictionary(&doc.trailer),
        objects,
        doc.objects.keys().copied().collect(),
    )
}

#[test]
fn delete_object_matches_the_earlier_implementation() {
    for seed in 0..300 {
        let mut doc = random_document(seed, 40);
        let mut old = doc.clone();
        let id = (seed as u32 % 43 + 1, 0);
        assert_eq!(doc.delete_object(id), old_delete_object(&mut old, id), "seed {seed}");
        // One deletion traverses once either way, so even unreachable objects match.
        let all = |d: &Document| d.objects.iter().map(|(id, o)| (*id, canonical(o))).collect::<Vec<_>>();
        assert_eq!(all(&doc), all(&old), "seed {seed}");
        assert_eq!(
            canonical_dictionary(&doc.trailer),
            canonical_dictionary(&old.trailer),
            "seed {seed}"
        );
    }
}

#[test]
fn delete_zero_length_streams_matches_deleting_one_stream_at_a_time() {
    for seed in 0..300 {
        let mut doc = random_document(seed, 40);
        let mut old = doc.clone();
        let ids = doc.delete_zero_length_streams();
        let expected: Vec<ObjectId> = old
            .objects
            .iter()
            .filter(|(_, o)| o.as_stream().map(|s| s.content.is_empty()).unwrap_or(false))
            .map(|(id, _)| *id)
            .collect();
        for id in &expected {
            old_delete_object(&mut old, *id);
        }
        assert_eq!(ids, expected, "seed {seed}");
        assert_eq!(reachable_state(&doc), reachable_state(&old), "seed {seed}");
    }
}

#[test]
fn dictionary_entries_keep_the_order_that_deleting_one_object_at_a_time_leaves() {
    // `Dictionary::remove` swaps the last entry into the removed one's place, so the order that
    // remains depends on which entries each deletion removes and in which order.
    for seed in 0..500 {
        let mut rng = Rng(seed * 31 + 1);
        let mut doc = Document::with_version("1.7");
        let mut dict = Dictionary::new();
        for key in 0..12 {
            let value = match rng.below(3) {
                0 => Object::Integer(key),
                _ => Object::Reference((2 + rng.below(5) as u32, 0)),
            };
            dict.set(format!("K{key}"), value);
        }
        doc.objects.insert((1, 0), Object::Dictionary(dict));
        for number in 2..7 {
            doc.objects
                .insert((number, 0), Object::Stream(Stream::new(Dictionary::new(), vec![])));
        }
        doc.trailer.set("Root", (1, 0));
        doc.max_id = 6;
        let mut old = doc.clone();
        let ids = doc.delete_zero_length_streams();
        for id in &ids {
            old_delete_object(&mut old, *id);
        }
        assert_eq!(
            canonical(&doc.objects[&(1, 0)]),
            canonical(&old.objects[&(1, 0)]),
            "seed {seed}"
        );
    }
}

/// A page tree of `n` pages under intermediate nodes, with link annotations between pages, page
/// references repeated in arrays, and random extra objects that reference pages.
fn random_paged_document(seed: u64, n: u32) -> Document {
    let mut rng = Rng(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) | 1);
    let mut doc = random_document(seed, 30);
    let first_page = 100;
    let page_id = |i: u32| (first_page + i, 0);
    let root: ObjectId = (90, 0);
    let groups = n.div_ceil(4);
    let mut root_kids = vec![];
    for g in 0..groups {
        let node = (91 + g % 8 + (g / 8) * 1000, 0);
        let members: Vec<u32> = (g * 4..((g + 1) * 4).min(n)).collect();
        let kids: Vec<Object> = members.iter().map(|i| Object::Reference(page_id(*i))).collect();
        doc.objects.insert(
            node,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Parent" => root, "Kids" => kids, "Count" => members.len() as i64,
            }),
        );
        root_kids.push(Object::Reference(node));
        for i in members {
            let target = page_id(rng.below(n as u64) as u32);
            let other = random_reference(&mut rng, 30);
            doc.objects.insert(
                page_id(i),
                Object::Dictionary(dictionary! {
                    "Type" => "Page", "Parent" => node,
                    "Annots" => vec![dictionary! { "Dest" => vec![Object::Reference(target), "Fit".into()] }.into()],
                    "Extra" => vec![Object::Reference(target), Object::Reference(target), other],
                    "Link" => Object::Reference(target),
                }),
            );
        }
    }
    doc.objects.insert(
        root,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => root_kids, "Count" => n as i64 }),
    );
    let catalog = (89, 0);
    doc.objects.insert(
        catalog,
        Object::Dictionary(
            dictionary! { "Type" => "Catalog", "Pages" => root, "Other" => random_reference(&mut rng, 30) },
        ),
    );
    doc.trailer.set("Root", catalog);
    doc.max_id = first_page + n + 8000;
    doc
}

#[test]
fn delete_pages_matches_deleting_one_page_at_a_time() {
    for seed in 0..300 {
        let n = 1 + (seed as u32 % 13);
        let mut doc = random_paged_document(seed, n);
        let mut old = doc.clone();
        let mut rng = Rng(seed + 7);
        // Page numbers in any order, with repeats and numbers past the last page.
        let page_numbers: Vec<u32> = (0..rng.below(n as u64 + 3))
            .map(|_| rng.below(n as u64 + 2) as u32)
            .collect();
        doc.delete_pages(&page_numbers);
        old_delete_pages(&mut old, &page_numbers);
        assert_eq!(
            reachable_state(&doc),
            reachable_state(&old),
            "seed {seed} {page_numbers:?}"
        );
        assert_eq!(doc.get_pages(), old.get_pages(), "seed {seed}");
    }
}

#[test]
fn deleting_many_pages_takes_linear_time() {
    // Deleting one page after another traversed the document once per page, which for half of
    // 20,000 pages took far longer than a test run.
    let n = 20_000;
    let mut doc = Document::with_version("1.7");
    let pages: ObjectId = (1, 0);
    let mut kids = vec![];
    for i in 0..n {
        let page = (i + 2, 0);
        doc.objects.insert(
            page,
            Object::Dictionary(dictionary! { "Type" => "Page", "Parent" => pages }),
        );
        kids.push(Object::Reference(page));
    }
    doc.objects.insert(
        pages,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => n as i64 }),
    );
    doc.objects.insert(
        (n + 2, 0),
        Object::Dictionary(dictionary! { "Type" => "Catalog", "Pages" => pages }),
    );
    doc.trailer.set("Root", (n + 2, 0));
    doc.max_id = n + 2;
    let page_numbers: Vec<u32> = (1..=n).step_by(2).collect();
    doc.delete_pages(&page_numbers);
    assert_eq!(doc.get_pages().len(), n as usize / 2);
    assert_eq!(doc.objects.len(), n as usize / 2 + 2);
}
