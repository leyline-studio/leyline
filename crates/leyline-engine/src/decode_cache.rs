//! In-memory cache of decoded source images (`docs/roadmap.md` phase 7).
//!
//! Decoding the RAW is by far the dominant cost of a preview render, and
//! the develop loop re-renders the same asset after every slider commit —
//! each new head revision invalidates the disk preview, so without this
//! cache every adjustment paid a full LibRaw decode. Source files never
//! change (non-destructive editing), so a decoded image stays valid for
//! the lifetime of the process; the cache only bounds memory, keeping the
//! most recently used decodes. Reproducibility is untouched: the cached
//! pixels are byte-for-byte what a fresh decode would return
//! (`docs/pipeline.md` §5), so ADR 0012 is not involved.

use std::collections::VecDeque;
use std::sync::Arc;

use leyline_core::AssetId;
use leyline_raw::{DecodeParams, RawImage};

/// A bounded most-recently-used cache of decoded images.
#[derive(Debug)]
pub struct DecodeCache {
    /// Most recently used first.
    entries: VecDeque<((AssetId, DecodeParams), Arc<RawImage>)>,
    capacity: usize,
}

impl DecodeCache {
    /// An empty cache holding at most `capacity` decoded images.
    pub fn new(capacity: usize) -> DecodeCache {
        DecodeCache {
            entries: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Returns the cached image for `(asset, params)`, calling `decode` on
    /// a miss and evicting the least recently used entry once full.
    pub fn get_or_insert_with<E>(
        &mut self,
        asset: AssetId,
        params: &DecodeParams,
        decode: impl FnOnce() -> Result<RawImage, E>,
    ) -> Result<Arc<RawImage>, E> {
        let key = (asset, params.clone());
        if let Some(position) = self.entries.iter().position(|(k, _)| *k == key) {
            let entry = self.entries.remove(position).expect("position is valid");
            self.entries.push_front(entry.clone());
            return Ok(entry.1);
        }
        let image = Arc::new(decode()?);
        if self.entries.len() == self.capacity {
            self.entries.pop_back();
        }
        self.entries.push_front((key, Arc::clone(&image)));
        Ok(image)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1×1 image whose sole pixel value identifies the decode.
    fn pixel(value: u8) -> RawImage {
        RawImage {
            width: 1,
            height: 1,
            bits: 8,
            data: vec![value; 3],
        }
    }

    #[test]
    fn hits_skip_the_decoder_and_misses_evict_the_oldest() {
        let mut cache = DecodeCache::new(2);
        let params = DecodeParams::default();
        let a = AssetId::new(1);
        let b = AssetId::new(2);
        let c = AssetId::new(3);

        let decoded = |v: u8| move || Ok::<_, ()>(pixel(v));
        let never = || Err(()); // a hit must not call the decoder

        assert_eq!(
            cache
                .get_or_insert_with(a, &params, decoded(1))
                .unwrap()
                .data[0],
            1
        );
        assert_eq!(
            cache
                .get_or_insert_with(b, &params, decoded(2))
                .unwrap()
                .data[0],
            2
        );
        assert_eq!(
            cache.get_or_insert_with(a, &params, never).unwrap().data[0],
            1
        );

        // `b` is now least recently used: inserting `c` evicts it.
        assert_eq!(
            cache
                .get_or_insert_with(c, &params, decoded(3))
                .unwrap()
                .data[0],
            3
        );
        assert!(cache.get_or_insert_with(b, &params, never).is_err());
    }

    #[test]
    fn different_params_are_different_entries() {
        let mut cache = DecodeCache::new(2);
        let a = AssetId::new(1);
        let full = DecodeParams::default();
        let half = DecodeParams {
            half_size: true,
            ..DecodeParams::default()
        };
        cache
            .get_or_insert_with(a, &full, || Ok::<_, ()>(pixel(1)))
            .unwrap();
        assert!(cache.get_or_insert_with(a, &half, || Err(())).is_err());
    }

    #[test]
    fn a_failed_decode_stores_nothing() {
        let mut cache = DecodeCache::new(2);
        let a = AssetId::new(1);
        let params = DecodeParams::default();
        assert!(cache.get_or_insert_with(a, &params, || Err(())).is_err());
        // The next call decodes again instead of serving a poisoned entry.
        assert_eq!(
            cache
                .get_or_insert_with(a, &params, || Ok::<_, ()>(pixel(9)))
                .unwrap()
                .data[0],
            9
        );
    }
}
