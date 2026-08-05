//! In-memory cache of source buffers — decoded images, and the display-size
//! proxies derived from them (`docs/roadmap.md` phase 7, ADR 0076).
//!
//! Decoding the RAW is by far the dominant cost of a preview render, and
//! the develop loop re-renders the same asset after every slider commit —
//! each new head revision invalidates the disk preview, so without this
//! cache every adjustment paid a full LibRaw decode. Source files never
//! change (non-destructive editing), so a decoded image stays valid for
//! the lifetime of the process; the cache only bounds memory, keeping the
//! most recently used entries.
//!
//! The proxy of ADR 0041 §1 — the decode reduced to the size class actually
//! being displayed — is the same kind of thing: a pure function of the
//! source file and the size asked for, so it is cached the same way and for
//! the same reason. It is what a live slider drag re-derives on every frame
//! (ADR 0074 §3), and rebuilding it costs more than the pipeline that
//! follows it.
//!
//! Reproducibility is untouched: the cached pixels are byte-for-byte what a
//! fresh decode — or a fresh reduction of one — would return
//! (`docs/pipeline.md` §5), so ADR 0012 is not involved.

use std::collections::VecDeque;
use std::sync::Arc;

use leyline_core::AssetId;
use leyline_raw::{DecodeParams, RawImage};

/// Memory a proxy list may hold before its least recently used entries are
/// dropped (ADR 0076 §2).
///
/// A budget in bytes rather than a count of entries, because the size
/// classes span 250×: at 64 MB this holds a dozen `Small` proxies (~4 MB
/// each, what the develop view drags) or a single `Large` one. The most
/// recent entry is always kept, whatever its size — evicting the buffer the
/// caller is about to use would defeat the cache without saving anything.
const PROXY_BUDGET_BYTES: usize = 64 * 1024 * 1024;

/// Everything a proxy is a function of: the decode it derives from, and the
/// size class it was reduced to (ADR 0076 §1).
type ProxyKey = (AssetId, DecodeParams, u32);

/// A reduced buffer and the scale factor the render must apply to its
/// pixel-denominated radii because of it (ADR 0041 §2).
type Proxy = (Arc<RawImage>, f32);

/// A bounded most-recently-used cache of decoded images and their proxies.
#[derive(Debug)]
pub struct DecodeCache {
    /// Most recently used first.
    entries: VecDeque<((AssetId, DecodeParams), Arc<RawImage>)>,
    capacity: usize,
    /// Reductions of `entries`. Most recently used first.
    proxies: VecDeque<(ProxyKey, Proxy)>,
    proxy_budget: usize,
}

impl DecodeCache {
    /// An empty cache holding at most `capacity` decoded images, and the
    /// default budget's worth of proxies.
    pub fn new(capacity: usize) -> DecodeCache {
        DecodeCache::with_proxy_budget(capacity, PROXY_BUDGET_BYTES)
    }

    /// [`DecodeCache::new`] with an explicit proxy budget, in bytes — the
    /// form the eviction tests use, since real proxies are megabytes.
    pub fn with_proxy_budget(capacity: usize, proxy_budget: usize) -> DecodeCache {
        DecodeCache {
            entries: VecDeque::with_capacity(capacity),
            capacity,
            proxies: VecDeque::new(),
            proxy_budget,
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

    /// Returns the decode of `(asset, params)` reduced to `max_edge`, with
    /// the scale factor that was applied — the pipeline's actual input on
    /// the preview path (ADR 0041 §1).
    ///
    /// `max_edge` of `None` — `PreviewKind::Full` — has no proxy: the decode
    /// itself comes back, at scale `1.0`. So does an image already small
    /// enough, without a second copy of it being made.
    ///
    /// A hit skips the reduction *and* the decode: the proxy is self
    /// sufficient, so it outlives the decoded buffer it came from when that
    /// one is evicted first.
    pub fn get_or_insert_proxy<E>(
        &mut self,
        asset: AssetId,
        params: &DecodeParams,
        max_edge: Option<u32>,
        decode: impl FnOnce() -> Result<RawImage, E>,
    ) -> Result<Proxy, E> {
        let Some(edge) = max_edge else {
            return Ok((self.get_or_insert_with(asset, params, decode)?, 1.0));
        };
        let key = (asset, params.clone(), edge);
        if let Some(position) = self.proxies.iter().position(|(k, _)| *k == key) {
            let entry = self.proxies.remove(position).expect("position is valid");
            self.proxies.push_front(entry.clone());
            return Ok(entry.1);
        }
        let decoded = self.get_or_insert_with(asset, params, decode)?;
        let (scaled, scale) = crate::downscale::downscale_to_fit(&decoded, edge);
        // At scale 1.0 the reduction returned a clone of its input; keep the
        // original instead, so a `Thumbnail` of an already-tiny image costs a
        // pointer rather than a second buffer.
        let proxy = if scale == 1.0 {
            decoded
        } else {
            Arc::new(scaled)
        };
        self.proxies.push_front((key, (Arc::clone(&proxy), scale)));
        self.trim_proxies();
        Ok((proxy, scale))
    }

    /// Drops least recently used proxies until the list fits its budget,
    /// always keeping the front one.
    fn trim_proxies(&mut self) {
        let mut total: usize = self.proxies.iter().map(|(_, (i, _))| i.data.len()).sum();
        while total > self.proxy_budget && self.proxies.len() > 1 {
            if let Some((_, (image, _))) = self.proxies.pop_back() {
                total -= image.data.len();
            }
        }
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

    /// A square 8-bit image big enough to actually be reduced, filled with
    /// `value` so any reduction of it is `value` again.
    fn square(edge: u32, value: u8) -> RawImage {
        RawImage {
            width: edge,
            height: edge,
            bits: 8,
            data: vec![value; (edge * edge * 3) as usize],
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

    /// The point of ADR 0076: the second frame of a drag pays neither the
    /// reduction nor the decode.
    #[test]
    fn a_proxy_hit_skips_the_reduction_and_the_decode() {
        let mut cache = DecodeCache::new(2);
        let params = DecodeParams::default();
        let a = AssetId::new(1);

        let (first, scale) = cache
            .get_or_insert_proxy(a, &params, Some(16), || Ok::<_, ()>(square(64, 7)))
            .unwrap();
        assert_eq!((first.width, first.height), (16, 16));
        assert_eq!(scale, 0.25);

        // The decoder is gone; only the cache can answer now.
        let (again, again_scale) = cache
            .get_or_insert_proxy(a, &params, Some(16), || Err(()))
            .unwrap();
        assert_eq!(again.data, first.data);
        assert_eq!(again_scale, scale);
    }

    /// The proxy outlives the buffer it came from: dropping the decode is
    /// what makes 4 MB worth keeping instead of 90.
    #[test]
    fn a_proxy_survives_the_eviction_of_its_decode() {
        let mut cache = DecodeCache::new(1);
        let params = DecodeParams::default();
        let a = AssetId::new(1);
        let b = AssetId::new(2);

        cache
            .get_or_insert_proxy(a, &params, Some(16), || Ok::<_, ()>(square(64, 7)))
            .unwrap();
        // `b`'s decode evicts `a`'s, the decode list holding exactly one.
        cache
            .get_or_insert_proxy(b, &params, Some(16), || Ok::<_, ()>(square(64, 9)))
            .unwrap();

        assert_eq!(
            cache
                .get_or_insert_proxy(a, &params, Some(16), || Err(()))
                .unwrap()
                .0
                .data[0],
            7
        );
    }

    /// Every term of the key is one: a different size class, or a different
    /// decode, is a different proxy — never a stale hit.
    #[test]
    fn each_size_class_and_each_decode_is_its_own_proxy() {
        let mut cache = DecodeCache::new(2);
        let a = AssetId::new(1);
        let full = DecodeParams::default();
        let half = DecodeParams {
            half_size: true,
            ..DecodeParams::default()
        };

        cache
            .get_or_insert_proxy(a, &full, Some(16), || Ok::<_, ()>(square(64, 7)))
            .unwrap();
        // Another size class re-reduces — from the cached decode, so the
        // decoder is still never called, but never from the 16 px proxy.
        let (wider, scale) = cache
            .get_or_insert_proxy(a, &full, Some(32), || Err(()))
            .unwrap();
        assert_eq!(
            ((wider.width, wider.height), scale),
            ((32, 32), 0.5),
            "another size class was served the first one's proxy"
        );
        assert!(
            cache
                .get_or_insert_proxy(a, &half, Some(16), || Err(()))
                .is_err(),
            "another decode served the first one's proxy"
        );
    }

    /// `PreviewKind::Full` has no `max_edge`, and an image already small
    /// enough is its own proxy — neither pays for a second buffer.
    #[test]
    fn full_resolution_and_small_enough_images_reuse_the_decode() {
        let mut cache = DecodeCache::new(2);
        let params = DecodeParams::default();
        let a = AssetId::new(1);
        let b = AssetId::new(2);

        let decoded = cache
            .get_or_insert_with(a, &params, || Ok::<_, ()>(square(64, 7)))
            .unwrap();
        let (proxy, scale) = cache
            .get_or_insert_proxy(a, &params, None, || Err(()))
            .unwrap();
        assert!(Arc::ptr_eq(&decoded, &proxy), "Full copied the decode");
        assert_eq!(scale, 1.0);

        let small = cache
            .get_or_insert_with(b, &params, || Ok::<_, ()>(square(8, 9)))
            .unwrap();
        let (proxy, scale) = cache
            .get_or_insert_proxy(b, &params, Some(16), || Err(()))
            .unwrap();
        assert!(
            Arc::ptr_eq(&small, &proxy),
            "an image that already fits was copied"
        );
        assert_eq!(scale, 1.0);
    }

    /// The budget is in bytes, and the entry just handed out is never the
    /// one dropped to honour it (ADR 0076 §2).
    #[test]
    fn proxies_are_evicted_by_weight_but_never_the_newest() {
        // Room for two 16×16 proxies (768 bytes each), not three.
        let mut cache = DecodeCache::with_proxy_budget(4, 2_000);
        let params = DecodeParams::default();
        let (a, b, c) = (AssetId::new(1), AssetId::new(2), AssetId::new(3));
        let proxy = |cache: &mut DecodeCache, asset, value| {
            cache
                .get_or_insert_proxy(asset, &params, Some(16), || Ok::<_, ()>(square(64, value)))
                .unwrap()
        };

        proxy(&mut cache, a, 1);
        proxy(&mut cache, b, 2);
        proxy(&mut cache, c, 3);
        assert_eq!(cache.proxies.len(), 2, "the budget was not honoured");
        assert_eq!(cache.proxies.front().unwrap().1.0.data[0], 3);

        // One proxy larger than the whole budget still gets kept, alone.
        let mut cache = DecodeCache::with_proxy_budget(4, 100);
        proxy(&mut cache, a, 1);
        proxy(&mut cache, b, 2);
        assert_eq!(cache.proxies.len(), 1);
        assert_eq!(cache.proxies.front().unwrap().1.0.data[0], 2);
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
