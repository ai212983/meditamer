const CHUNK_LATENCY_SAMPLE_CAP: usize = 32;

pub(super) struct ChunkLatencySamples {
    values: [u16; CHUNK_LATENCY_SAMPLE_CAP],
    len: usize,
    dropped: u32,
    max_ms: u32,
}

impl ChunkLatencySamples {
    pub(super) fn new() -> Self {
        Self {
            values: [0; CHUNK_LATENCY_SAMPLE_CAP],
            len: 0,
            dropped: 0,
            max_ms: 0,
        }
    }

    pub(super) fn max_ms(&self) -> u32 {
        self.max_ms
    }

    pub(super) fn len(&self) -> usize {
        self.len
    }

    pub(super) fn dropped(&self) -> u32 {
        self.dropped
    }
}

pub(super) fn record_chunk_latency_sample(samples: &mut ChunkLatencySamples, latency_ms: u32) {
    samples.max_ms = samples.max_ms.max(latency_ms);
    let latency_u16 = latency_ms.min(u16::MAX as u32) as u16;
    if samples.len < CHUNK_LATENCY_SAMPLE_CAP {
        samples.values[samples.len] = latency_u16;
        samples.len += 1;
    } else {
        samples.dropped = samples.dropped.saturating_add(1);
    }
}

pub(super) fn chunk_latency_quantiles(samples: &ChunkLatencySamples) -> (u32, u32) {
    if samples.len == 0 {
        return (0, 0);
    }
    let mut sorted = [0u16; CHUNK_LATENCY_SAMPLE_CAP];
    sorted[..samples.len].copy_from_slice(&samples.values[..samples.len]);
    sorted[..samples.len].sort_unstable();

    let p50_idx = ((samples.len - 1) * 50) / 100;
    let p95_idx = ((samples.len - 1) * 95) / 100;
    (sorted[p50_idx] as u32, sorted[p95_idx] as u32)
}
