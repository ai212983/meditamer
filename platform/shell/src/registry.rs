use heapless::Vec;

use super::types::{
    ProviderGeneration, ProviderId, ProviderToken, SurfaceDefinition, SurfaceId, SurfaceRef,
    SurfaceSpec,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
struct ProviderRecord {
    last_generation: ProviderGeneration,
    id: ProviderId,
    active: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistrationError {
    EmptyProvider,
    ProviderAlreadyActive(ProviderToken),
    ProviderCapacity,
    GenerationExhausted(ProviderId),
    DuplicateSurfaceId {
        registered: SurfaceRef,
        requested: SurfaceId,
    },
    DuplicateSurfaceIdInBatch(SurfaceId),
    SurfaceCapacity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolveError {
    UnknownProvider(ProviderId),
    StaleProviderGeneration {
        active: Option<ProviderToken>,
        requested: ProviderToken,
    },
    UnknownSurface(SurfaceRef),
}

pub struct SurfaceRegistry<const PROVIDER_CAPACITY: usize, const SURFACE_CAPACITY: usize> {
    providers: Vec<ProviderRecord, PROVIDER_CAPACITY>,
    definitions: Vec<SurfaceDefinition, SURFACE_CAPACITY>,
}

impl<const PROVIDER_CAPACITY: usize, const SURFACE_CAPACITY: usize>
    SurfaceRegistry<PROVIDER_CAPACITY, SURFACE_CAPACITY>
{
    pub const fn new() -> Self {
        Self {
            providers: Vec::new(),
            definitions: Vec::new(),
        }
    }

    pub fn register_provider(
        &mut self,
        provider: ProviderId,
        surfaces: &[SurfaceSpec],
    ) -> Result<ProviderToken, RegistrationError> {
        if surfaces.is_empty() {
            return Err(RegistrationError::EmptyProvider);
        }

        let provider_index = self
            .providers
            .iter()
            .position(|record| record.id == provider);
        let generation = match provider_index {
            Some(index) if self.providers[index].active => {
                return Err(RegistrationError::ProviderAlreadyActive(
                    ProviderToken::issued(provider, self.providers[index].last_generation),
                ));
            }
            Some(index) => self.providers[index]
                .last_generation
                .0
                .checked_add(1)
                .map(ProviderGeneration)
                .ok_or(RegistrationError::GenerationExhausted(provider))?,
            None if self.providers.len() == PROVIDER_CAPACITY => {
                return Err(RegistrationError::ProviderCapacity);
            }
            None => ProviderGeneration(1),
        };

        if self.definitions.len().saturating_add(surfaces.len()) > SURFACE_CAPACITY {
            return Err(RegistrationError::SurfaceCapacity);
        }
        for (index, surface) in surfaces.iter().enumerate() {
            if let Some(registered) = self
                .definitions
                .iter()
                .find(|registered| registered.surface.id == surface.id)
            {
                return Err(RegistrationError::DuplicateSurfaceId {
                    registered: registered.surface,
                    requested: surface.id,
                });
            }
            if surfaces[..index]
                .iter()
                .any(|earlier| earlier.id == surface.id)
            {
                return Err(RegistrationError::DuplicateSurfaceIdInBatch(surface.id));
            }
        }

        let token = ProviderToken::issued(provider, generation);
        match provider_index {
            Some(index) => {
                self.providers[index].last_generation = generation;
                self.providers[index].active = true;
            }
            None => self
                .providers
                .push(ProviderRecord {
                    last_generation: generation,
                    id: provider,
                    active: true,
                })
                .map_err(|_| RegistrationError::ProviderCapacity)?,
        }
        for surface in surfaces {
            self.definitions
                .push(surface.with_owner(token))
                .map_err(|_| RegistrationError::SurfaceCapacity)?;
        }
        Ok(token)
    }

    pub fn resolve(&self, surface: SurfaceRef) -> Result<&SurfaceDefinition, ResolveError> {
        self.validate_owner(surface.owner)?;
        self.definitions
            .iter()
            .find(|definition| definition.surface == surface)
            .ok_or(ResolveError::UnknownSurface(surface))
    }

    pub fn validate_owner(&self, owner: ProviderToken) -> Result<(), ResolveError> {
        let Some(record) = self.providers.iter().find(|record| record.id == owner.id) else {
            return Err(ResolveError::UnknownProvider(owner.id));
        };
        let active = record
            .active
            .then(|| ProviderToken::issued(record.id, record.last_generation));
        if active == Some(owner) {
            Ok(())
        } else {
            Err(ResolveError::StaleProviderGeneration {
                active,
                requested: owner,
            })
        }
    }

    pub fn unregister_provider(&mut self, owner: ProviderToken) -> usize {
        let before = self.definitions.len();
        let mut index = 0;
        while index < self.definitions.len() {
            if self.definitions[index].surface.owner == owner {
                self.definitions.remove(index);
            } else {
                index += 1;
            }
        }
        if let Some(record) = self
            .providers
            .iter_mut()
            .find(|record| record.id == owner.id && record.last_generation == owner.generation)
        {
            record.active = false;
        }
        before - self.definitions.len()
    }

    pub fn definition_len(&self) -> usize {
        self.definitions.len()
    }

    pub fn provider_len(&self) -> usize {
        self.providers.len()
    }
}

impl<const PROVIDER_CAPACITY: usize, const SURFACE_CAPACITY: usize> Default
    for SurfaceRegistry<PROVIDER_CAPACITY, SURFACE_CAPACITY>
{
    fn default() -> Self {
        Self::new()
    }
}
