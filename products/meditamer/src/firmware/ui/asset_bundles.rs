//! Source-backed phase payload declarations for Clock and Ambient Home.
//!
//! These describe required payload layouts, not currently reserved PSRAM.
//! The shared boot canvas, retained sky/sun pack, optional Mountain store,
//! allocator charges and dynamic Clock row work are accounted separately.

use residency_policy::{Bundle, BundleId, BundlePart, ResourceId};

/// Clock fixed source maps, stationary gray/packed base, and frame staging.
/// Dynamic row workspaces and allocator charges are not included.
pub const CLOCK_FIXED_PARTS: [BundlePart; 12] = clock_parts();
pub const CLOCK_FIXED: Bundle<'static> = Bundle {
    id: BundleId(1),
    version: 1,
    parts: &CLOCK_FIXED_PARTS,
};

const fn clock_parts() -> [BundlePart; 12] {
    let mut parts = [BundlePart {
        resource: ResourceId(1),
        bytes: 1,
        align: 4,
    }; 12];
    let mut index = 0;
    while index < clock_assets::MAP_LENGTHS.len() {
        parts[index] = BundlePart {
            resource: ResourceId(1 + index as u16),
            bytes: clock_assets::MAP_LENGTHS[index],
            align: 4,
        };
        index += 1;
    }
    parts[9] = BundlePart {
        resource: ResourceId(10),
        bytes: 360_000, // stationary L8 base
        align: 4,
    };
    parts[10] = BundlePart {
        resource: ResourceId(11),
        bytes: 45_000, // stationary packed base
        align: 4,
    };
    parts[11] = BundlePart {
        resource: ResourceId(12),
        bytes: 45_000, // frame staging
        align: 4,
    };
    parts
}

/// Ambient Home without the optional Mountain overlay.
pub const AMBIENT_CORE_PARTS: [BundlePart; 1] = [BundlePart {
    resource: ResourceId(20),
    bytes: 45_000, // packed frame staging
    align: 4,
}];
pub const AMBIENT_CORE: Bundle<'static> = Bundle {
    id: BundleId(2),
    version: 1,
    parts: &AMBIENT_CORE_PARTS,
};

/// Ambient Home when its optional, screen-lifetime Mountain overlay is held.
pub const AMBIENT_WITH_MOUNTAIN_PARTS: [BundlePart; 2] = [
    AMBIENT_CORE_PARTS[0],
    BundlePart {
        resource: ResourceId(21),
        bytes: 40_350, // ink and eligibility planes
        align: 4,
    },
];
pub const AMBIENT_WITH_MOUNTAIN: Bundle<'static> = Bundle {
    id: BundleId(3),
    version: 1,
    parts: &AMBIENT_WITH_MOUNTAIN_PARTS,
};
