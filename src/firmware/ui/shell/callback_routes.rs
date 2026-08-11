const ROUTE_INDEX_BITS: u32 = 3;
const ROUTE_INDEX_MASK: u32 = (1 << ROUTE_INDEX_BITS) - 1;
const MAX_ROUTE_CAPACITY: usize = ROUTE_INDEX_MASK as usize;
const MAX_ROUTE_GENERATION: u32 = u32::MAX >> ROUTE_INDEX_BITS;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CallbackRoute(u32);

impl CallbackRoute {
    pub(crate) const fn encoded(self) -> u32 {
        self.0
    }

    pub(crate) const fn from_encoded(encoded: u32) -> Option<Self> {
        if encoded & ROUTE_INDEX_MASK == 0 || encoded >> ROUTE_INDEX_BITS == 0 {
            None
        } else {
            Some(Self(encoded))
        }
    }

    const fn index(self) -> usize {
        ((self.0 & ROUTE_INDEX_MASK) - 1) as usize
    }

    const fn generation(self) -> u32 {
        self.0 >> ROUTE_INDEX_BITS
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CallbackRouteError {
    Capacity,
    GenerationExhausted,
    InvalidCapacity,
    StaleRoute,
}

#[derive(Clone, Copy)]
struct RouteSlot<T: Copy> {
    generation: u32,
    value: Option<T>,
    enabled: bool,
}

impl<T: Copy> RouteSlot<T> {
    const EMPTY: Self = Self {
        generation: 0,
        value: None,
        enabled: false,
    };
}

pub(crate) struct CallbackRouteTable<T: Copy, const CAPACITY: usize> {
    slots: [RouteSlot<T>; CAPACITY],
}

impl<T: Copy, const CAPACITY: usize> CallbackRouteTable<T, CAPACITY> {
    pub(crate) const fn new() -> Self {
        Self {
            slots: [RouteSlot::EMPTY; CAPACITY],
        }
    }

    pub(crate) fn claim(&mut self, value: T) -> Result<CallbackRoute, CallbackRouteError> {
        if CAPACITY == 0 || CAPACITY > MAX_ROUTE_CAPACITY {
            return Err(CallbackRouteError::InvalidCapacity);
        }
        let Some((index, slot)) = self
            .slots
            .iter_mut()
            .enumerate()
            .find(|(_, slot)| slot.value.is_none())
        else {
            return Err(CallbackRouteError::Capacity);
        };
        let generation = slot
            .generation
            .checked_add(1)
            .filter(|generation| *generation <= MAX_ROUTE_GENERATION)
            .ok_or(CallbackRouteError::GenerationExhausted)?;
        slot.generation = generation;
        slot.value = Some(value);
        slot.enabled = false;
        Ok(CallbackRoute(
            (generation << ROUTE_INDEX_BITS) | (index as u32 + 1),
        ))
    }

    pub(crate) fn enable(&mut self, route: CallbackRoute) -> Result<(), CallbackRouteError> {
        self.slot_mut(route)?.enabled = true;
        Ok(())
    }

    pub(crate) fn disable(&mut self, route: CallbackRoute) -> Result<(), CallbackRouteError> {
        self.slot_mut(route)?.enabled = false;
        Ok(())
    }

    pub(crate) fn release(&mut self, route: CallbackRoute) -> Result<(), CallbackRouteError> {
        let slot = self.slot_mut(route)?;
        slot.enabled = false;
        slot.value = None;
        Ok(())
    }

    pub(crate) fn resolve(&self, route: CallbackRoute) -> Option<T> {
        let slot = self.slots.get(route.index())?;
        (slot.enabled && slot.generation == route.generation())
            .then_some(slot.value)
            .flatten()
    }

    pub(crate) fn any_value(&self, mut matches: impl FnMut(T) -> bool) -> bool {
        self.slots
            .iter()
            .filter_map(|slot| slot.value)
            .any(&mut matches)
    }

    fn slot_mut(&mut self, route: CallbackRoute) -> Result<&mut RouteSlot<T>, CallbackRouteError> {
        let slot = self
            .slots
            .get_mut(route.index())
            .ok_or(CallbackRouteError::StaleRoute)?;
        if slot.generation != route.generation() || slot.value.is_none() {
            return Err(CallbackRouteError::StaleRoute);
        }
        Ok(slot)
    }
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;

    #[test]
    fn disabled_routes_do_not_resolve_and_capacity_is_explicit() {
        let mut table = CallbackRouteTable::<u8, 2>::new();
        let first = table.claim(11).unwrap();
        let second = table.claim(22).unwrap();
        assert_eq!(table.resolve(first), None);
        table.enable(first).unwrap();
        assert_eq!(table.resolve(first), Some(11));
        assert_eq!(table.claim(33), Err(CallbackRouteError::Capacity));
        table.disable(first).unwrap();
        assert_eq!(table.resolve(first), None);
        assert_eq!(table.resolve(second), None);
        assert!(table.any_value(|value| value == 11));
        table.release(first).unwrap();
        assert!(!table.any_value(|value| value == 11));
    }

    #[test]
    fn reused_slot_rejects_the_old_route_generation() {
        let mut table = CallbackRouteTable::<u8, 1>::new();
        let first = table.claim(11).unwrap();
        table.enable(first).unwrap();
        table.release(first).unwrap();
        let second = table.claim(22).unwrap();
        table.enable(second).unwrap();

        assert_ne!(first, second);
        assert_eq!(table.resolve(first), None);
        assert_eq!(table.enable(first), Err(CallbackRouteError::StaleRoute));
        assert_eq!(table.resolve(second), Some(22));
    }

    #[test]
    fn four_owner_handoff_peak_is_bounded_and_reusable() {
        let mut table = CallbackRouteTable::<u8, 4>::new();
        let routes = [
            table.claim(1).unwrap(),
            table.claim(2).unwrap(),
            table.claim(3).unwrap(),
            table.claim(4).unwrap(),
        ];
        assert_eq!(table.claim(5), Err(CallbackRouteError::Capacity));
        for (route, value) in routes.into_iter().zip(1_u8..=4) {
            table.enable(route).unwrap();
            assert_eq!(table.resolve(route), Some(value));
        }

        table.release(routes[1]).unwrap();
        let replacement = table.claim(5).unwrap();
        table.enable(replacement).unwrap();
        assert_eq!(table.resolve(routes[1]), None);
        assert_eq!(table.resolve(replacement), Some(5));
    }
}
