use fst::Streamer;
use std::mem;
use std::collections::BinaryHeap;
use fst::map::Keys;
use schema::Term;
use core::SegmentReader;
use std::cmp::Ordering;


static EMPTY: [u8; 0] = [];

#[derive(PartialEq, Eq, Debug)]
struct HeapItem {
    term: Term,
    segment_ord: usize,
}

impl PartialOrd for HeapItem {
    fn partial_cmp(&self, other: &HeapItem) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapItem {
    fn cmp(&self, other: &HeapItem) -> Ordering {
        (&other.term, &other.segment_ord).cmp(&(&self.term, &self.segment_ord))
    }
}

/// Given a list of sorted term streams,
/// returns an iterator over sorted unique terms.
///
/// The item yield is actually a pair with
/// - the term
/// - a slice with the ordinal of the segments containing
/// the terms.
pub struct TermIterator<'a> {
    key_streams: Vec<Keys<'a>>,
    heap: BinaryHeap<HeapItem>,
    // Buffer hosting the list of segment ordinals containing
    // the current term.
    current_term: Term,
    current_segment_ords: Vec<usize>,
}

impl<'a> TermIterator<'a> {
    fn new(key_streams: Vec<Keys<'a>>) -> TermIterator<'a> {
        let key_streams_len = key_streams.len();
        let mut term_iterator = TermIterator {
            key_streams: key_streams,
            heap: BinaryHeap::new(),
            current_term: Term::from(&EMPTY[..]),
            current_segment_ords: vec![],
        };
        for segment_ord in 0..key_streams_len {
            term_iterator.push_next_segment_el(segment_ord);
        }
        term_iterator
    }

    fn push_next_segment_el(&mut self, segment_ord: usize) {
        self.current_segment_ords.push(segment_ord);
        if let Some(term) = self.key_streams[segment_ord].next() {
            self.heap.push(HeapItem {
                term: Term::from(term),
                segment_ord: segment_ord,
            });
        }
    }
}

impl<'a, 'f> Streamer<'a> for TermIterator<'f> {
    type Item = (&'a Term, &'a [usize]);

    fn next(&'a mut self) -> Option<Self::Item> {
        self.current_segment_ords.clear();
        self.heap
            .pop()
            .map(move |mut head| {
                mem::swap(&mut self.current_term, &mut head.term);
                self.push_next_segment_el(head.segment_ord);
                loop {
                    match self.heap.peek() {
                        Some(&ref next_heap_it) if next_heap_it.term == self.current_term => {}
                        _ => {
                            break;
                        }
                    }
                    let next_heap_it = self.heap
                                           .pop()
                                           .expect("This is only reached if an element was \
                                                    peeked beforehand.");
                    self.push_next_segment_el(next_heap_it.segment_ord);
                }
                (&self.current_term, self.current_segment_ords.as_slice())
            })
    }
}

impl<'a> From<&'a [SegmentReader]> for TermIterator<'a> {
    fn from(segment_readers: &'a [SegmentReader]) -> TermIterator<'a> {
        TermIterator::new(segment_readers.iter()
                                         .map(|reader| reader.term_infos().keys())
                                         .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schema::{SchemaBuilder, Document, TEXT};
    use core::Index;

    #[test]
    fn test_term_iterator() {
        let mut schema_builder = SchemaBuilder::default();
        let text_field = schema_builder.add_text_field("text", TEXT);
        let index = Index::create_in_ram(schema_builder.build());
        {
            let mut index_writer = index.writer_with_num_threads(1, 40_000_000).unwrap();
            {
                // writing the segment
                {
                    let mut doc = Document::default();
                    doc.add_text(text_field, "a b d f");
                    index_writer.add_document(doc).unwrap();
                }
                index_writer.commit().unwrap();
            }
            {
                // writing the segment
                {
                    let mut doc = Document::default();
                    doc.add_text(text_field, "a b c d f");
                    index_writer.add_document(doc).unwrap();
                }
                index_writer.commit().unwrap();
            }
            {
                // writing the segment
                {
                    let mut doc = Document::default();
                    doc.add_text(text_field, "e f");
                    index_writer.add_document(doc).unwrap();
                }
                index_writer.commit().unwrap();
            }
        }
        let searcher = index.searcher();
        let mut term_it = searcher.terms();
        {

            let (term, segments) = term_it.next().unwrap();
            assert_eq!(term.value(), "a".as_bytes());
            let expected_segments = [0, 1];
            assert_eq!(segments, &expected_segments);

        }
        {
            let (term, segments): (&Term, &[usize]) = term_it.next().unwrap();
            assert_eq!(term.value(), "b".as_bytes());
            let expected_segments = [0, 1];
            assert_eq!(segments, &expected_segments);
        }
        {
            let (ref term, ref segments) = term_it.next().unwrap();
            assert_eq!(term.value(), "c".as_bytes());
            let expected_segments = [1];
            assert_eq!(segments, &expected_segments);
        }
        {
            let (term, segments) = term_it.next().unwrap();
            assert_eq!(term.value(), "d".as_bytes());
            let expected_segments = [0, 1];
            assert_eq!(segments, &expected_segments);
        }
        {
            let (term, segments) = term_it.next().unwrap();
            assert_eq!(term.value(), "e".as_bytes());
            let expected_segments = [2];
            assert_eq!(segments, &expected_segments);
        }
        {
            let (term, segments) = term_it.next().unwrap();
            assert_eq!(term.value(), "f".as_bytes());
            let expected_segments = [0, 1, 2];
            assert_eq!(segments, &expected_segments);
        }
    }

}