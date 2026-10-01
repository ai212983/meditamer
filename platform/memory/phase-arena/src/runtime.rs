use allocator_api2::alloc::{Allocator, Layout};
use allocator_api2::vec::Vec;
use core::ptr::NonNull;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartShape {
    Contiguous,
    Segmented { max_segment_bytes: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartRequest {
    pub bytes: usize,
    pub align: usize,
    pub shape: PartShape,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeBundleError {
    Empty,
    InvalidSize,
    InvalidAlignment,
    Overflow,
    OverBound { requested: usize, bound: usize },
    MetadataAllocationFailed,
    InvalidPartLayout { index: usize },
    PartAllocationFailed { index: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessError {
    Overflow,
    OutOfBounds {
        offset: usize,
        len: usize,
        capacity: usize,
    },
}

#[derive(Debug)]
struct Segment {
    ptr: NonNull<u8>,
    layout: Layout,
}

// SAFETY: sole ownership of the allocation moves with the value.
unsafe impl Send for Segment {}
// SAFETY: shared borrows expose only immutable bytes.
unsafe impl Sync for Segment {}

#[derive(Clone, Copy, Debug)]
struct PartHeader {
    len: usize,
    seg_base: usize,
    seg_count: usize,
    seg_unit: usize,
    segmented: bool,
}

#[derive(Debug)]
enum Entry {
    Part(PartHeader),
    Seg(Segment),
}

pub struct RuntimeBundle<A: Allocator> {
    entries: Vec<Entry, A>,
    parts: usize,
    total: usize,
}

impl<A: Allocator> core::fmt::Debug for RuntimeBundle<A> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RuntimeBundle")
            .field("parts", &self.parts)
            .field("total", &self.total)
            .finish()
    }
}

fn checked_end(offset: usize, len: usize, capacity: usize) -> Result<usize, AccessError> {
    let end = offset.checked_add(len).ok_or(AccessError::Overflow)?;
    if end > capacity {
        return Err(AccessError::OutOfBounds {
            offset,
            len,
            capacity,
        });
    }
    Ok(end)
}

fn free_segments<A: Allocator>(alloc: &A, entries: &[Entry]) {
    for entry in entries {
        if let Entry::Seg(segment) = entry {
            // SAFETY: each entry denotes a live block from this allocator, freed once.
            unsafe {
                alloc.deallocate(segment.ptr, segment.layout);
            }
        }
    }
}

impl<A: Allocator> RuntimeBundle<A> {
    pub fn try_new_zeroed(
        requests: &[PartRequest],
        max_total_bytes: usize,
        alloc: A,
    ) -> Result<Self, RuntimeBundleError> {
        if requests.is_empty() {
            return Err(RuntimeBundleError::Empty);
        }
        let mut total = 0usize;
        let mut segments = 0usize;
        for request in requests {
            if request.bytes == 0 {
                return Err(RuntimeBundleError::InvalidSize);
            }
            if request.align == 0 || !request.align.is_power_of_two() {
                return Err(RuntimeBundleError::InvalidAlignment);
            }
            let mut count = 1usize;
            if let PartShape::Segmented { max_segment_bytes } = request.shape {
                if max_segment_bytes == 0 {
                    return Err(RuntimeBundleError::InvalidSize);
                }
                count = request.bytes.div_ceil(max_segment_bytes);
            }
            total = total
                .checked_add(request.bytes)
                .ok_or(RuntimeBundleError::Overflow)?;
            segments = segments
                .checked_add(count)
                .ok_or(RuntimeBundleError::Overflow)?;
        }
        if total > max_total_bytes {
            return Err(RuntimeBundleError::OverBound {
                requested: total,
                bound: max_total_bytes,
            });
        }
        let entry_count = requests
            .len()
            .checked_add(segments)
            .ok_or(RuntimeBundleError::Overflow)?;
        let mut entries = Vec::new_in(alloc);
        entries
            .try_reserve_exact(entry_count)
            .map_err(|_| RuntimeBundleError::MetadataAllocationFailed)?;
        let mut seg_base = requests.len();
        for request in requests {
            let (count, unit, segmented) = match request.shape {
                PartShape::Contiguous => (1, request.bytes, false),
                PartShape::Segmented { max_segment_bytes } => (
                    request.bytes.div_ceil(max_segment_bytes),
                    max_segment_bytes,
                    true,
                ),
            };
            entries.push(Entry::Part(PartHeader {
                len: request.bytes,
                seg_base,
                seg_count: count,
                seg_unit: unit,
                segmented,
            }));
            seg_base += count;
        }
        for (index, request) in requests.iter().enumerate() {
            let (count, unit, len) = match &entries[index] {
                Entry::Part(header) => (header.seg_count, header.seg_unit, header.len),
                Entry::Seg(_) => unreachable!(),
            };
            let mut start = 0usize;
            for _ in 0..count {
                let seg_len = (len - start).min(unit);
                let layout = Layout::from_size_align(seg_len, request.align).map_err(|_| {
                    free_segments(entries.allocator(), &entries);
                    RuntimeBundleError::InvalidPartLayout { index }
                })?;
                let memory = entries.allocator().allocate_zeroed(layout).map_err(|_| {
                    free_segments(entries.allocator(), &entries);
                    RuntimeBundleError::PartAllocationFailed { index }
                })?;
                entries.push(Entry::Seg(Segment {
                    ptr: memory.cast(),
                    layout,
                }));
                start += seg_len;
            }
        }
        Ok(Self {
            entries,
            parts: requests.len(),
            total,
        })
    }

    pub fn part_count(&self) -> usize {
        self.parts
    }

    pub fn total_bytes(&self) -> usize {
        self.total
    }

    pub fn part_len(&self, index: usize) -> Option<usize> {
        self.header(index).map(|header| header.len)
    }

    pub fn is_segmented(&self, index: usize) -> Option<bool> {
        self.header(index).map(|header| header.segmented)
    }

    pub fn segment_count(&self, index: usize) -> Option<usize> {
        self.header(index).map(|header| header.seg_count)
    }

    pub fn segment_len(&self, index: usize, segment: usize) -> Option<usize> {
        let header = self.header(index)?;
        if segment >= header.seg_count {
            return None;
        }
        Some(self.segment(header, segment).layout.size())
    }

    pub fn part(&self, index: usize) -> Option<&[u8]> {
        let header = self.header(index)?;
        if header.segmented {
            return None;
        }
        let segment = self.segment(header, 0);
        // SAFETY: segment owned by self; shared borrow forbids overlap.
        Some(unsafe { core::slice::from_raw_parts(segment.ptr.as_ptr(), segment.layout.size()) })
    }

    pub fn part_mut(&mut self, index: usize) -> Option<&mut [u8]> {
        let header = self.header(index)?;
        if header.segmented {
            return None;
        }
        let segment = self.segment_mut(header, 0);
        // SAFETY: segment owned by self; exclusive borrow excludes overlap.
        Some(unsafe {
            core::slice::from_raw_parts_mut(segment.ptr.as_ptr(), segment.layout.size())
        })
    }

    pub fn read_at(&self, index: usize, offset: usize, dst: &mut [u8]) -> Result<(), AccessError> {
        let header = match self.header(index) {
            Some(header) => header,
            None => {
                return Err(AccessError::OutOfBounds {
                    offset,
                    len: dst.len(),
                    capacity: 0,
                });
            }
        };
        checked_end(offset, dst.len(), header.len)?;
        let mut filled = 0;
        while filled < dst.len() {
            let position = offset + filled;
            let s = position / header.seg_unit;
            let within = position % header.seg_unit;
            let segment = self.segment(header, s);
            let take = (segment.layout.size() - within).min(dst.len() - filled);
            // SAFETY: checked range lies inside this owned segment.
            let bytes =
                unsafe { core::slice::from_raw_parts(segment.ptr.as_ptr(), segment.layout.size()) };
            dst[filled..filled + take].copy_from_slice(&bytes[within..within + take]);
            filled += take;
        }
        Ok(())
    }

    pub fn write_at(&mut self, index: usize, offset: usize, src: &[u8]) -> Result<(), AccessError> {
        let header = match self.header(index) {
            Some(header) => header,
            None => {
                return Err(AccessError::OutOfBounds {
                    offset,
                    len: src.len(),
                    capacity: 0,
                });
            }
        };
        checked_end(offset, src.len(), header.len)?;
        let mut written = 0;
        while written < src.len() {
            let position = offset + written;
            let s = position / header.seg_unit;
            let within = position % header.seg_unit;
            let segment = self.segment_mut(header, s);
            let take = (segment.layout.size() - within).min(src.len() - written);
            // SAFETY: checked range lies inside this exclusively owned segment.
            let bytes = unsafe {
                core::slice::from_raw_parts_mut(segment.ptr.as_ptr(), segment.layout.size())
            };
            bytes[within..within + take].copy_from_slice(&src[written..written + take]);
            written += take;
        }
        Ok(())
    }

    pub fn segment_at(&self, index: usize, offset: usize, len: usize) -> Option<&[u8]> {
        let header = self.header(index)?;
        if !header.segmented {
            return None;
        }
        if len == 0 {
            return if offset <= header.len {
                Some(&[])
            } else {
                None
            };
        }
        let end = offset.checked_add(len)?;
        if end > header.len {
            return None;
        }
        let s = offset / header.seg_unit;
        let within = offset % header.seg_unit;
        let segment = self.segment(header, s);
        if len > segment.layout.size() - within {
            return None;
        }
        // SAFETY: checked range lies inside this owned segment.
        let bytes =
            unsafe { core::slice::from_raw_parts(segment.ptr.as_ptr(), segment.layout.size()) };
        Some(&bytes[within..within + len])
    }

    pub fn segment_at_mut(&mut self, index: usize, offset: usize, len: usize) -> Option<&mut [u8]> {
        let header = self.header(index)?;
        if !header.segmented {
            return None;
        }
        if len == 0 {
            return if offset <= header.len {
                Some(&mut [])
            } else {
                None
            };
        }
        let end = offset.checked_add(len)?;
        if end > header.len {
            return None;
        }
        let s = offset / header.seg_unit;
        let within = offset % header.seg_unit;
        let segment = self.segment_mut(header, s);
        if len > segment.layout.size() - within {
            return None;
        }
        // SAFETY: checked range lies inside this exclusively owned segment.
        let bytes =
            unsafe { core::slice::from_raw_parts_mut(segment.ptr.as_ptr(), segment.layout.size()) };
        Some(&mut bytes[within..within + len])
    }

    pub fn view(&self) -> RuntimeView<'_, A> {
        RuntimeView { owner: self }
    }

    fn header(&self, index: usize) -> Option<PartHeader> {
        if index >= self.parts {
            return None;
        }
        match &self.entries[index] {
            Entry::Part(header) => Some(*header),
            Entry::Seg(_) => None,
        }
    }

    fn segment(&self, header: PartHeader, s: usize) -> &Segment {
        match &self.entries[header.seg_base + s] {
            Entry::Seg(segment) => segment,
            Entry::Part(_) => unreachable!(),
        }
    }

    fn segment_mut(&mut self, header: PartHeader, s: usize) -> &mut Segment {
        match &mut self.entries[header.seg_base + s] {
            Entry::Seg(segment) => segment,
            Entry::Part(_) => unreachable!(),
        }
    }
}

impl<A: Allocator> Drop for RuntimeBundle<A> {
    fn drop(&mut self) {
        free_segments(self.entries.allocator(), &self.entries);
    }
}

pub struct RuntimeView<'a, A: Allocator> {
    owner: &'a RuntimeBundle<A>,
}

impl<'a, A: Allocator> RuntimeView<'a, A> {
    pub fn part_count(&self) -> usize {
        self.owner.part_count()
    }

    pub fn total_bytes(&self) -> usize {
        self.owner.total_bytes()
    }

    pub fn part_len(&self, index: usize) -> Option<usize> {
        self.owner.part_len(index)
    }

    pub fn is_segmented(&self, index: usize) -> Option<bool> {
        self.owner.is_segmented(index)
    }

    pub fn part(&self, index: usize) -> Option<&[u8]> {
        self.owner.part(index)
    }

    pub fn read_at(&self, index: usize, offset: usize, dst: &mut [u8]) -> Result<(), AccessError> {
        self.owner.read_at(index, offset, dst)
    }

    pub fn segment_at(&self, index: usize, offset: usize, len: usize) -> Option<&[u8]> {
        self.owner.segment_at(index, offset, len)
    }

    pub fn segment_count(&self, index: usize) -> Option<usize> {
        self.owner.segment_count(index)
    }

    pub fn segment_len(&self, index: usize, segment: usize) -> Option<usize> {
        self.owner.segment_len(index, segment)
    }
}
