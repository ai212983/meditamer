//! Paper-anchored cellular sand model.
//!
//! The 2x2 transition table is the Devlin-Schuster rule. Medinote adds a small,
//! explicitly custom free-surface relaxation pass because the base rule left
//! visually unacceptable thin V-shaped legs in the product geometry. Product
//! pacing and firmware ownership stay outside this module.

use super::geometry::cubic_bezier_half_width_cells;

pub const TOPPLE_SCALE: u16 = 1_000;
pub const DEFAULT_TOPPLE_PER_MILLE: u16 = 750;
pub const MEDINOTE_GRAINS: usize = 4_500;
pub const DEFAULT_REPOSE_RUN_CELLS: u8 = 3;
pub const MAX_REPOSE_RUN_CELLS: u8 = 4;
pub const DEFAULT_SURFACE_RELAX_INTERVAL: u8 = 4;
pub const MAX_SURFACE_RELAX_INTERVAL: u8 = 32;

const MEDINOTE_HALF_WIDTH: i32 = 58;
const MEDINOTE_HALF_HEIGHT: i32 = 85;
const MEDINOTE_GRID_WIDTH: usize = (MEDINOTE_HALF_WIDTH * 2 + 3) as usize;
const MEDINOTE_GRID_HEIGHT: usize = (MEDINOTE_HALF_HEIGHT * 2 + 3) as usize;
const MAX_GRID_WIDTH: usize = MEDINOTE_GRID_WIDTH;
const MAX_GRID_HEIGHT: usize = MEDINOTE_GRID_HEIGHT;
const MAX_GRID_CELLS: usize = MAX_GRID_WIDTH * MAX_GRID_HEIGHT;
const BITMAP_WORDS: usize = MAX_GRID_CELLS.div_ceil(32);

const TOP_LEFT: u8 = 0b0001;
const TOP_RIGHT: u8 = 0b0010;
const BOTTOM_LEFT: u8 = 0b0100;
const BOTTOM_RIGHT: u8 = 0b1000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Geometry {
    Open,
    Medinote,
    PaperReference,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BlockDirections {
    Uniform(GravityDirection),
    #[cfg(feature = "hourglass-block-gravity")]
    Weighted {
        vertical: GravityDirection,
        horizontal: GravityDirection,
        horizontal_weight: u32,
        total_weight: u32,
    },
}

impl BlockDirections {
    fn for_block(self, _seed: u32, _generation: u32, _x: usize, _y: usize) -> GravityDirection {
        match self {
            Self::Uniform(direction) => direction,
            #[cfg(feature = "hourglass-block-gravity")]
            Self::Weighted {
                vertical,
                horizontal,
                horizontal_weight,
                total_weight,
            } => {
                let sample = mix32(
                    _seed
                        ^ 0xD1B5_4A35
                        ^ _generation.wrapping_mul(0x9E37_79B9)
                        ^ (_x as u32).wrapping_mul(0x85EB_CA6B)
                        ^ (_y as u32).wrapping_mul(0xC2B2_AE35),
                );
                if sample % total_weight < horizontal_weight {
                    horizontal
                } else {
                    vertical
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Bitmap {
    words: [u32; BITMAP_WORDS],
}

impl Default for Bitmap {
    fn default() -> Self {
        Self {
            words: [0; BITMAP_WORDS],
        }
    }
}

impl Bitmap {
    fn contains(&self, index: usize) -> bool {
        self.words[index / 32] & (1 << (index % 32)) != 0
    }

    fn insert(&mut self, index: usize) {
        self.words[index / 32] |= 1 << (index % 32);
    }

    fn remove(&mut self, index: usize) {
        self.words[index / 32] &= !(1 << (index % 32));
    }
}

/// One occupied sand cell in glass-local coordinates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SandCell {
    pub x: i16,
    pub y: i16,
}

/// Cardinal lattice direction selected for one Margolus generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GravityDirection {
    Down,
    Left,
    Up,
    Right,
}

impl GravityDirection {
    const fn vertical_sign(self) -> Option<i32> {
        match self {
            Self::Down => Some(1),
            Self::Up => Some(-1),
            Self::Left | Self::Right => None,
        }
    }
}

/// Result of one generation, including exact mass crossing the bulb boundary.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PaperStep {
    pub changed: bool,
    pub upper_to_lower: u8,
    pub lower_to_upper: u8,
}

impl PaperStep {
    const fn crossings(self) -> u8 {
        self.upper_to_lower + self.lower_to_upper
    }

    fn record(&mut self, update: Self) {
        self.changed |= update.changed;
        self.upper_to_lower += update.upper_to_lower;
        self.lower_to_upper += update.lower_to_upper;
    }
}

/// Allocation-free fixed-down cellular model shared by host and firmware.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaperCellular {
    sand: Bitmap,
    walls: Bitmap,
    width: u16,
    height: u16,
    generation: u32,
    seed: u32,
    topple_per_mille: u16,
    repose_run_cells: u8,
    surface_relax_interval: u8,
    geometry: Geometry,
}

impl PaperCellular {
    /// Curved Medinote glass with an unpaced throat and a compact upper fill.
    pub fn medinote_unpaced(grains: usize, topple_per_mille: u16, seed: u32) -> Option<Self> {
        Self::medinote_unpaced_with_repose(grains, topple_per_mille, seed, DEFAULT_REPOSE_RUN_CELLS)
    }

    /// Curved Medinote glass with a host-tunable free-surface repose stencil.
    ///
    /// A run of zero disables the added surface pass and leaves only the
    /// Devlin-Schuster transitions. A non-zero run compares topmost grains in
    /// columns `repose_run_cells` apart and topples an over-steep backed grain.
    /// Rycroft, Wong, and Bazant motivate the desired surface avalanching, but
    /// this bounded column stencil is a Medinote-specific rule, not a direct
    /// implementation of their Spot Model. The run selects the visual angle of
    /// repose while the cadence bounds its cost and supply rate.
    pub fn medinote_unpaced_with_repose(
        grains: usize,
        topple_per_mille: u16,
        seed: u32,
        repose_run_cells: u8,
    ) -> Option<Self> {
        Self::medinote_unpaced_with_surface(
            grains,
            topple_per_mille,
            seed,
            repose_run_cells,
            DEFAULT_SURFACE_RELAX_INTERVAL,
        )
    }

    /// Curved Medinote glass with explicit repose geometry and cadence.
    pub fn medinote_unpaced_with_surface(
        grains: usize,
        topple_per_mille: u16,
        seed: u32,
        repose_run_cells: u8,
        surface_relax_interval: u8,
    ) -> Option<Self> {
        if repose_run_cells > MAX_REPOSE_RUN_CELLS
            || surface_relax_interval == 0
            || surface_relax_interval > MAX_SURFACE_RELAX_INTERVAL
        {
            return None;
        }
        let mut model = Self::empty_centered(
            MEDINOTE_GRID_WIDTH,
            MEDINOTE_GRID_HEIGHT,
            topple_per_mille,
            seed,
        )?;
        model.repose_run_cells = repose_run_cells;
        model.surface_relax_interval = surface_relax_interval;
        model.geometry = Geometry::Medinote;
        model.add_hourglass_walls(
            MEDINOTE_HALF_WIDTH,
            MEDINOTE_HALF_HEIGHT,
            medinote_half_width,
        );
        model.fill_upper(grains, MEDINOTE_HALF_HEIGHT, medinote_half_width)?;
        Some(model)
    }

    /// The paper's 61x61 test size with a simple straight-sided hourglass.
    pub fn paper_reference(grains: usize, topple_per_mille: u16, seed: u32) -> Option<Self> {
        const HALF_SPAN: i32 = 30;
        let mut model = Self::empty_centered(61, 61, topple_per_mille, seed)?;
        model.geometry = Geometry::PaperReference;
        model.add_hourglass_walls(HALF_SPAN - 1, HALF_SPAN - 1, reference_half_width);
        model.fill_upper(grains, HALF_SPAN - 1, reference_half_width)?;
        Some(model)
    }

    fn empty_centered(
        width: usize,
        height: usize,
        topple_per_mille: u16,
        seed: u32,
    ) -> Option<Self> {
        if width < 3
            || height < 3
            || width > MAX_GRID_WIDTH
            || height > MAX_GRID_HEIGHT
            || width.is_multiple_of(2)
            || height.is_multiple_of(2)
            || topple_per_mille > TOPPLE_SCALE
        {
            return None;
        }
        Some(Self {
            sand: Bitmap::default(),
            walls: Bitmap::default(),
            width: width as u16,
            height: height as u16,
            generation: 0,
            seed,
            topple_per_mille,
            repose_run_cells: 0,
            surface_relax_interval: 1,
            geometry: Geometry::Open,
        })
    }

    pub const fn width(&self) -> usize {
        self.width as usize
    }

    pub const fn height(&self) -> usize {
        self.height as usize
    }

    pub const fn generation(&self) -> u32 {
        self.generation
    }

    pub fn sand_count(&self) -> usize {
        self.sand_cells().count()
    }

    pub fn sand_cells(&self) -> SandCells<'_> {
        SandCells {
            model: self,
            active_index: 0,
        }
    }

    /// Advance one generation under fixed downward gravity.
    pub fn step_down(&mut self) -> bool {
        self.step(GravityDirection::Down, None).changed
    }

    /// Advance one generation in a cardinal direction.
    ///
    /// `crossing_budget` limits the number of sand cells that may cross the
    /// horizontal bulb boundary during this generation. A 2x2 transition is
    /// accepted whole or rejected whole, preserving the paper's block rule.
    pub fn step(&mut self, direction: GravityDirection, crossing_budget: Option<u8>) -> PaperStep {
        self.step_blocks(
            BlockDirections::Uniform(direction),
            Some(direction),
            crossing_budget,
        )
    }

    /// Advance one generation with spatially distributed neighboring gravity
    /// directions while retaining one shared, disjoint Margolus partition.
    #[cfg(feature = "hourglass-block-gravity")]
    pub(crate) fn step_weighted(
        &mut self,
        vertical: GravityDirection,
        horizontal: GravityDirection,
        horizontal_weight: u32,
        total_weight: u32,
        surface_direction: GravityDirection,
        crossing_budget: Option<u8>,
    ) -> PaperStep {
        debug_assert!(horizontal_weight <= total_weight && total_weight > 0);
        self.step_blocks(
            BlockDirections::Weighted {
                vertical,
                horizontal,
                horizontal_weight,
                total_weight,
            },
            Some(surface_direction),
            crossing_budget,
        )
    }

    fn step_blocks(
        &mut self,
        directions: BlockDirections,
        surface_direction: Option<GravityDirection>,
        crossing_budget: Option<u8>,
    ) -> PaperStep {
        let (offset_x, offset_y) = partition_offset(self.generation);
        let mut result = PaperStep::default();
        let mut remaining_crossings = crossing_budget;
        let mut y = offset_y;
        while y + 1 < self.height() {
            let mut x = offset_x;
            while x + 1 < self.width() {
                let direction = directions.for_block(self.seed, self.generation, x, y);
                let update = self.update_block(x, y, direction, remaining_crossings);
                if let Some(remaining) = &mut remaining_crossings {
                    *remaining -= update.crossings();
                }
                result.record(update);
                x += 2;
            }
            y += 2;
        }
        if self.repose_run_cells > 0
            && self
                .generation
                .is_multiple_of(u32::from(self.surface_relax_interval))
        {
            if let Some(gravity_y) = surface_direction.and_then(GravityDirection::vertical_sign) {
                result.record(self.relax_surface(gravity_y, remaining_crossings));
            }
        }
        self.generation = self.generation.wrapping_add(1);
        result
    }

    /// Move over-steep free-surface grains through a disjoint void exchange.
    ///
    /// Source columns are partitioned so their source/destination intervals do
    /// not overlap. The upper surface may descend by at most one cell across
    /// the configured horizontal run; a larger step topples one backed surface
    /// grain onto the lower neighboring surface.
    fn relax_surface(&mut self, gravity_y: i32, crossing_budget: Option<u8>) -> PaperStep {
        let run = usize::from(self.repose_run_cells);
        if run == 0 {
            return PaperStep::default();
        }

        let horizontal_period = run * 2 + 1;
        let surface_step = self.generation / u32::from(self.surface_relax_interval);
        let phase_x = surface_step as usize % horizontal_period;
        let mut result = PaperStep::default();
        let mut remaining_crossings = crossing_budget;

        for source_x in run..self.width().saturating_sub(run) {
            if source_x % horizontal_period != phase_x {
                continue;
            }
            let Some(source_y) = self.source_surface_y(source_x, gravity_y) else {
                continue;
            };
            let source = self.index(source_x, source_y);
            let left = self.surface_destination(source_x, source_y, -(run as i32), gravity_y);
            let right = self.surface_destination(source_x, source_y, run as i32, gravity_y);
            let target = match (left, right) {
                (Some((left, left_drop)), Some((right, right_drop))) => {
                    if left_drop > right_drop
                        || (left_drop == right_drop && self.surface_choice_for(source_x, source_y))
                    {
                        left
                    } else {
                        right
                    }
                }
                (Some((target, _)), None) | (None, Some((target, _))) => target,
                (None, None) => continue,
            };
            let crossings = crossings_for_cells(self.height(), source_y, target / MAX_GRID_WIDTH);
            if remaining_crossings.is_some_and(|budget| crossings.crossings() > budget) {
                continue;
            }

            debug_assert!(self.sand.contains(source) && !self.walls.contains(source));
            debug_assert!(!self.sand.contains(target) && !self.walls.contains(target));
            self.sand.remove(source);
            self.sand.insert(target);
            if let Some(remaining) = &mut remaining_crossings {
                *remaining -= crossings.crossings();
            }
            result.record(PaperStep {
                changed: true,
                ..crossings
            });
        }
        result
    }

    fn surface_destination(
        &self,
        source_x: usize,
        source_y: usize,
        offset_x: i32,
        gravity_y: i32,
    ) -> Option<(usize, usize)> {
        let movement = offset_x.signum();
        if !self.has_surface_backing(source_x, source_y, -movement, gravity_y) {
            return None;
        }
        let above_y = source_y as i32 - gravity_y;
        if !(0..i32::from(self.height)).contains(&above_y) {
            return None;
        }
        let above_source = self.index(source_x, above_y as usize);
        if self.sand.contains(above_source) || self.walls.contains(above_source) {
            return None;
        }

        let target_x = (source_x as i32 + offset_x) as usize;
        let target_surface = self.source_surface_y(target_x, gravity_y);
        let target_y = match target_surface {
            Some(surface_y) => usize::try_from(surface_y as i32 - gravity_y).ok()?,
            None => usize::try_from(source_y as i32 + gravity_y).ok()?,
        };
        let drop = (target_y as i32 - source_y as i32) * gravity_y;
        let drop = usize::try_from(drop).ok()?;
        if (target_surface.is_some() && drop <= 1) || !self.is_open_interior(target_x, target_y) {
            return None;
        }
        Some((self.index(target_x, target_y), drop))
    }

    fn source_surface_y(&self, x: usize, gravity_y: i32) -> Option<usize> {
        let throat_y = self.height() / 2;
        if gravity_y > 0 {
            (1..throat_y).find(|y| self.sand.contains(self.index(x, *y)))
        } else {
            ((throat_y + 1)..self.height().saturating_sub(1))
                .rev()
                .find(|y| self.sand.contains(self.index(x, *y)))
        }
    }

    fn has_surface_backing(&self, x: usize, y: usize, outward: i32, gravity_y: i32) -> bool {
        [(outward, 0), (outward, gravity_y), (0, gravity_y)]
            .into_iter()
            .any(|(offset_x, offset_y)| {
                let neighbor_x = x as i32 + offset_x;
                let neighbor_y = y as i32 + offset_y;
                neighbor_x >= 0
                    && neighbor_y >= 0
                    && neighbor_x < i32::from(self.width)
                    && neighbor_y < i32::from(self.height)
                    && self
                        .sand
                        .contains(self.index(neighbor_x as usize, neighbor_y as usize))
            })
    }

    fn is_open_interior(&self, x: usize, y: usize) -> bool {
        let index = self.index(x, y);
        if self.sand.contains(index) || self.walls.contains(index) {
            return false;
        }
        let local_x = x as i32 - i32::from(self.width) / 2;
        let local_y = y as i32 - i32::from(self.height) / 2;
        match self.geometry {
            Geometry::Open => true,
            Geometry::Medinote => {
                local_y.abs() <= MEDINOTE_HALF_HEIGHT
                    && local_x.abs() <= medinote_half_width(local_y)
            }
            Geometry::PaperReference => {
                local_y.abs() <= 29 && local_x.abs() <= reference_half_width(local_y)
            }
        }
    }

    fn add_hourglass_walls(&mut self, half_width: i32, half_height: i32, profile: fn(i32) -> i32) {
        let cap_y = half_height + 1;
        for x in -(half_width + 1)..=(half_width + 1) {
            self.set_wall_local(x, -cap_y);
            self.set_wall_local(x, cap_y);
        }
        for y in -half_height..=half_height {
            let wall_x = profile(y) + 1;
            self.set_wall_local(-wall_x, y);
            self.set_wall_local(wall_x, y);
        }
    }

    fn fill_upper(
        &mut self,
        grains: usize,
        half_height: i32,
        profile: fn(i32) -> i32,
    ) -> Option<()> {
        if grains == 0 {
            return None;
        }
        let mut placed = 0;
        let mut y = -1;
        while y >= -half_height && placed < grains {
            let half_width = profile(y);
            self.place_sand_local(0, y)?;
            placed += 1;
            let mut distance = 1;
            while distance <= half_width && placed < grains {
                for x in [-distance, distance] {
                    if placed == grains {
                        break;
                    }
                    self.place_sand_local(x, y)?;
                    placed += 1;
                }
                distance += 1;
            }
            y -= 1;
        }
        (placed == grains).then_some(())
    }

    fn update_block(
        &mut self,
        x: usize,
        y: usize,
        direction: GravityDirection,
        crossing_budget: Option<u8>,
    ) -> PaperStep {
        let indices = [
            self.index(x, y),
            self.index(x + 1, y),
            self.index(x, y + 1),
            self.index(x + 1, y + 1),
        ];
        let sand_pattern = pattern_for(&indices, &self.sand);
        let occupied = block_pattern(&indices, &self.sand, &self.walls);
        let wall_pattern = pattern_for(&indices, &self.walls);
        let topple = self.topple_for(x, y);
        let next = transition(occupied, topple, direction);
        if next == occupied || wall_pattern & !next != 0 {
            return PaperStep::default();
        }
        let next_sand = next & !wall_pattern;
        let crossings = crossings_for_block(self.height(), y, sand_pattern, next_sand);
        if crossing_budget.is_some_and(|budget| crossings.crossings() > budget) {
            return PaperStep::default();
        }

        for (bit, index) in [TOP_LEFT, TOP_RIGHT, BOTTOM_LEFT, BOTTOM_RIGHT]
            .into_iter()
            .zip(indices)
        {
            self.sand.remove(index);
            if next & bit != 0 && wall_pattern & bit == 0 {
                self.sand.insert(index);
            }
        }
        PaperStep {
            changed: true,
            ..crossings
        }
    }

    fn topple_for(&self, x: usize, y: usize) -> bool {
        if self.topple_per_mille == 0 {
            return false;
        }
        if self.topple_per_mille == TOPPLE_SCALE {
            return true;
        }
        let entropy = mix32(
            self.seed
                ^ self.generation.wrapping_mul(0x9E37_79B9)
                ^ (x as u32).wrapping_mul(0x85EB_CA6B)
                ^ (y as u32).wrapping_mul(0xC2B2_AE35),
        );
        entropy % u32::from(TOPPLE_SCALE) < u32::from(self.topple_per_mille)
    }

    fn surface_choice_for(&self, x: usize, y: usize) -> bool {
        let entropy = mix32(
            self.seed
                ^ 0xA511_E9B3
                ^ self.generation.wrapping_mul(0x63D8_3595)
                ^ (x as u32).wrapping_mul(0x9E37_79B9)
                ^ (y as u32).wrapping_mul(0x85EB_CA6B),
        );
        entropy & 1 == 0
    }

    fn index(&self, x: usize, y: usize) -> usize {
        debug_assert!(x < self.width() && y < self.height());
        y * MAX_GRID_WIDTH + x
    }

    fn local_to_index(&self, x: i32, y: i32) -> Option<usize> {
        let grid_x = x + i32::from(self.width) / 2;
        let grid_y = y + i32::from(self.height) / 2;
        if grid_x < 0
            || grid_y < 0
            || grid_x >= i32::from(self.width)
            || grid_y >= i32::from(self.height)
        {
            return None;
        }
        Some(self.index(grid_x as usize, grid_y as usize))
    }

    fn set_wall_local(&mut self, x: i32, y: i32) {
        let index = self
            .local_to_index(x, y)
            .expect("hourglass wall fits configured grid");
        self.sand.remove(index);
        self.walls.insert(index);
    }

    fn place_sand_local(&mut self, x: i32, y: i32) -> Option<()> {
        let index = self.local_to_index(x, y)?;
        if self.walls.contains(index) || self.sand.contains(index) {
            return None;
        }
        self.sand.insert(index);
        Some(())
    }

    #[cfg(test)]
    fn contains_sand_local(&self, x: i32, y: i32) -> bool {
        self.local_to_index(x, y)
            .is_some_and(|index| self.sand.contains(index))
    }

    #[cfg(test)]
    fn contains_wall_local(&self, x: i32, y: i32) -> bool {
        self.local_to_index(x, y)
            .is_some_and(|index| self.walls.contains(index))
    }
}

#[derive(Clone)]
pub struct SandCells<'a> {
    model: &'a PaperCellular,
    active_index: usize,
}

impl Iterator for SandCells<'_> {
    type Item = SandCell;

    fn next(&mut self) -> Option<Self::Item> {
        let active_cells = self.model.width() * self.model.height();
        while self.active_index < active_cells {
            let active_index = self.active_index;
            self.active_index += 1;
            let x = active_index % self.model.width();
            let y = active_index / self.model.width();
            if self.model.sand.contains(self.model.index(x, y)) {
                return Some(SandCell {
                    x: (x as i32 - i32::from(self.model.width) / 2) as i16,
                    y: (y as i32 - i32::from(self.model.height) / 2) as i16,
                });
            }
        }
        None
    }
}

pub fn medinote_half_width(y: i32) -> i32 {
    // A point-width opening is sealed for a 2x2 block rule: every partition
    // touching its centre also contains a wall. Three interior cells are the
    // smallest symmetric opening that remains usable in all four phases.
    cubic_bezier_half_width_cells(y, 0, MEDINOTE_HALF_WIDTH, MEDINOTE_HALF_HEIGHT).max(1)
}

fn reference_half_width(y: i32) -> i32 {
    1 + y.abs() * 27 / 29
}

fn partition_offset(generation: u32) -> (usize, usize) {
    match generation % 4 {
        0 => (0, 0),
        1 => (1, 1),
        2 => (0, 1),
        _ => (1, 0),
    }
}

fn block_pattern(indices: &[usize; 4], sand: &Bitmap, walls: &Bitmap) -> u8 {
    pattern_for(indices, sand) | pattern_for(indices, walls)
}

fn pattern_for(indices: &[usize; 4], bitmap: &Bitmap) -> u8 {
    [TOP_LEFT, TOP_RIGHT, BOTTOM_LEFT, BOTTOM_RIGHT]
        .into_iter()
        .zip(indices.iter().copied())
        .fold(0, |pattern, (bit, index)| {
            if bitmap.contains(index) {
                pattern | bit
            } else {
                pattern
            }
        })
}

fn crossings_for_block(height: usize, y: usize, before: u8, after: u8) -> PaperStep {
    if y != height / 2 {
        return PaperStep::default();
    }
    let upper_before = (before & (TOP_LEFT | TOP_RIGHT)).count_ones() as u8;
    let upper_after = (after & (TOP_LEFT | TOP_RIGHT)).count_ones() as u8;
    match upper_before.cmp(&upper_after) {
        core::cmp::Ordering::Greater => PaperStep {
            upper_to_lower: upper_before - upper_after,
            ..PaperStep::default()
        },
        core::cmp::Ordering::Less => PaperStep {
            lower_to_upper: upper_after - upper_before,
            ..PaperStep::default()
        },
        core::cmp::Ordering::Equal => PaperStep::default(),
    }
}

fn crossings_for_cells(height: usize, before_y: usize, after_y: usize) -> PaperStep {
    let throat_y = height / 2;
    match (before_y <= throat_y, after_y <= throat_y) {
        (true, false) => PaperStep {
            upper_to_lower: 1,
            ..PaperStep::default()
        },
        (false, true) => PaperStep {
            lower_to_upper: 1,
            ..PaperStep::default()
        },
        _ => PaperStep::default(),
    }
}

const fn transition(pattern: u8, topple: bool, direction: GravityDirection) -> u8 {
    match direction {
        GravityDirection::Down => transition_down(pattern, topple),
        GravityDirection::Right => {
            rotate_counter_clockwise(transition_down(rotate_clockwise(pattern), topple))
        }
        GravityDirection::Up => rotate_half(transition_down(rotate_half(pattern), topple)),
        GravityDirection::Left => {
            rotate_clockwise(transition_down(rotate_counter_clockwise(pattern), topple))
        }
    }
}

const fn rotate_clockwise(pattern: u8) -> u8 {
    ((pattern & TOP_LEFT) << 1)
        | ((pattern & TOP_RIGHT) << 2)
        | ((pattern & BOTTOM_RIGHT) >> 1)
        | ((pattern & BOTTOM_LEFT) >> 2)
}

const fn rotate_counter_clockwise(pattern: u8) -> u8 {
    ((pattern & TOP_LEFT) << 2)
        | ((pattern & BOTTOM_LEFT) << 1)
        | ((pattern & BOTTOM_RIGHT) >> 2)
        | ((pattern & TOP_RIGHT) >> 1)
}

const fn rotate_half(pattern: u8) -> u8 {
    rotate_clockwise(rotate_clockwise(pattern))
}

/// Figure 2's fixed-down before/after patterns. Figure labels are not encoded
/// because the published figure skips `(g)`; the bit patterns are definitive.
const fn transition_down(pattern: u8, topple: bool) -> u8 {
    match pattern {
        0b0001 => 0b0100,
        0b0010 => 0b1000,
        0b0011 => 0b1100,
        0b1011 => 0b1110,
        0b0111 => 0b1101,
        0b0110 => 0b1100,
        0b1001 => 0b1100,
        0b1010 if topple => 0b1100,
        0b0101 if topple => 0b1100,
        _ => pattern,
    }
}

fn mix32(mut value: u32) -> u32 {
    value ^= value >> 16;
    value = value.wrapping_mul(0x7FEB_352D);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846C_A68B);
    value ^ (value >> 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_sixteen_patterns_match_the_figure_rule() {
        const NO_TOPPLE: [u8; 16] = [0, 4, 8, 12, 4, 5, 12, 13, 8, 12, 10, 14, 12, 13, 14, 15];
        const TOPPLE: [u8; 16] = [0, 4, 8, 12, 4, 12, 12, 13, 8, 12, 12, 14, 12, 13, 14, 15];
        for pattern in 0..16 {
            assert_eq!(transition_down(pattern, false), NO_TOPPLE[pattern as usize]);
            assert_eq!(transition_down(pattern, true), TOPPLE[pattern as usize]);
        }
    }

    #[cfg(feature = "hourglass-block-gravity")]
    #[test]
    fn weighted_block_directions_are_spatial_deterministic_and_mirrored() {
        let right = BlockDirections::Weighted {
            vertical: GravityDirection::Down,
            horizontal: GravityDirection::Right,
            horizontal_weight: 1,
            total_weight: 2,
        };
        let left = BlockDirections::Weighted {
            vertical: GravityDirection::Down,
            horizontal: GravityDirection::Left,
            horizontal_weight: 1,
            total_weight: 2,
        };
        let mut horizontal = 0;
        let mut vertical = 0;
        for generation in 0..4 {
            let (offset_x, offset_y) = partition_offset(generation);
            for y in (offset_y..32).step_by(2) {
                for x in (offset_x..32).step_by(2) {
                    let right_direction = right.for_block(7, generation, x, y);
                    let left_direction = left.for_block(7, generation, x, y);
                    match right_direction {
                        GravityDirection::Right => {
                            horizontal += 1;
                            assert_eq!(left_direction, GravityDirection::Left);
                        }
                        GravityDirection::Down => {
                            vertical += 1;
                            assert_eq!(left_direction, GravityDirection::Down);
                        }
                        _ => panic!("weighted selector returned an unrelated direction"),
                    }
                    assert_eq!(right_direction, right.for_block(7, generation, x, y));
                }
            }
        }
        assert!(horizontal > 400 && vertical > 400);
    }

    #[test]
    fn partition_offsets_follow_the_published_four_step_cycle() {
        assert_eq!(partition_offset(0), (0, 0));
        assert_eq!(partition_offset(1), (1, 1));
        assert_eq!(partition_offset(2), (0, 1));
        assert_eq!(partition_offset(3), (1, 0));
        assert_eq!(partition_offset(4), (0, 0));
    }

    #[test]
    fn a_transition_cannot_move_a_wall_cell() {
        let mut model = PaperCellular::empty_centered(5, 5, TOPPLE_SCALE, 1).unwrap();
        model.set_wall_local(-2, -2);
        model.place_sand_local(-1, -2).unwrap();
        let before = model.sand_count();
        assert!(!model.step_down());
        assert!(model.contains_wall_local(-2, -2));
        assert!(model.contains_sand_local(-1, -2));
        assert_eq!(model.sand_count(), before);
    }

    #[test]
    fn medinote_seed_fits_and_conserves_mass() {
        let mut model =
            PaperCellular::medinote_unpaced(MEDINOTE_GRAINS, DEFAULT_TOPPLE_PER_MILLE, 7).unwrap();
        assert_eq!(model.sand_count(), MEDINOTE_GRAINS);
        for _ in 0..300 {
            model.step_down();
            assert_eq!(model.sand_count(), MEDINOTE_GRAINS);
        }
        assert!(model.sand_cells().any(|cell| cell.y > 0));
        assert!(model.sand_cells().all(|cell| {
            i32::from(cell.y).abs() <= MEDINOTE_HALF_HEIGHT
                && i32::from(cell.x).abs() <= medinote_half_width(i32::from(cell.y))
        }));
    }

    #[test]
    fn surface_relaxation_moves_an_oversteep_grain_once() {
        let mut model = PaperCellular::empty_centered(9, 9, TOPPLE_SCALE, 1).unwrap();
        model.repose_run_cells = 2;
        model.generation = 2;
        model.place_sand_local(-2, -1).unwrap();
        model.place_sand_local(-3, -1).unwrap();
        model.place_sand_local(-4, -1).unwrap();

        assert!(model.relax_surface(1, None).changed);
        assert!(!model.contains_sand_local(-2, -1));
        assert!(model.contains_sand_local(0, 0));
        assert_eq!(model.sand_count(), 3);
    }

    #[test]
    fn surface_at_repose_does_not_topple() {
        let mut model = PaperCellular::empty_centered(9, 9, TOPPLE_SCALE, 1).unwrap();
        model.repose_run_cells = 2;
        model.generation = 2;
        model.place_sand_local(-2, -1).unwrap();
        model.place_sand_local(-3, -1).unwrap();
        model.place_sand_local(-4, -1).unwrap();
        model.place_sand_local(0, -1).unwrap();

        assert!(!model.relax_surface(1, None).changed);
        assert!(model.contains_sand_local(-2, -1));
        assert!(model.contains_sand_local(0, -1));
        assert!(!model.contains_sand_local(0, 0));
        assert_eq!(model.sand_count(), 4);
    }

    #[test]
    fn surface_relaxation_activates_after_drainage_starts() {
        let mut model = PaperCellular::medinote_unpaced_with_repose(
            MEDINOTE_GRAINS,
            DEFAULT_TOPPLE_PER_MILLE,
            1,
            0,
        )
        .unwrap();
        for _ in 0..300 {
            model.step_down();
        }
        model.repose_run_cells = DEFAULT_REPOSE_RUN_CELLS;

        let changed = (0..20).any(|_| {
            let changed = model.relax_surface(1, None).changed;
            model.generation = model.generation.wrapping_add(1);
            changed
        });
        assert!(changed);
    }

    #[test]
    fn initial_medinote_seed_is_stable_on_its_first_closed_throat_generation() {
        let mut model =
            PaperCellular::medinote_unpaced(MEDINOTE_GRAINS, DEFAULT_TOPPLE_PER_MILLE, 1).unwrap();
        let before = model;

        let result = model.step(GravityDirection::Down, Some(0));

        assert!(!result.changed);
        assert_eq!(model.sand, before.sand);
    }
}

#[cfg(test)]
mod determinism_tests;
