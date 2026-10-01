//! Medinote's product app kinds over the shell's shared [`ProductApps`]
//! registry.
//!
//! The kind enum only names "which app" -- each variant's *state* lives with
//! its own app (the `hourglass` crate's `HourglassModel`,
//! `apps::counter`'s `CounterState`, each held by the target the same static
//! way), matching the plan's "app runtimes own app state, app input, and app
//! timing" at the type level without forcing every app's state through one
//! shared shape. There is no separate id-to-kind lookup: registration binds
//! each [`AppDescriptor`] directly to its [`MedinoteApp`], and
//! [`ProductApps::app_for_surface`] resolves an active surface straight to
//! the kind.

use shell::apps::{AppRegistration, ProductApps, RegisteredApp};

/// Which Medinote app a resolved surface belongs to. `SurfaceRole::AppRoot`
/// alone cannot answer this once more than one app exists. App activation
/// follows the committed surface-instance itself, independent of whether the
/// destination came from KEY, a console command, startup, or a future input
/// source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MedinoteApp {
    Hourglass,
    Counter,
    #[cfg(feature = "network-controls")]
    Network,
}

/// Stable metadata for one Medinote app: the shared descriptor instantiated
/// with [`MedinoteApp`].
pub type AppDescriptor = shell::apps::AppDescriptor<MedinoteApp>;

/// One Medinote descriptor paired with its shell-issued provider token, as
/// pushed into [`MedinoteApps`] after registration.
pub type MedinoteRegistration = AppRegistration<MedinoteApp>;

/// One registered Medinote app: its kind plus its resolved surfaces.
pub type MedinoteRegisteredApp = RegisteredApp<MedinoteApp>;

/// Fixed capacity for every Medinote app (Hourglass, Counter, and the
/// feature-gated Network app).
pub const MAX_APPS: usize = 3;

/// Medinote's resolved apps: Hourglass, Counter, and optionally Network.
pub type MedinoteApps = ProductApps<MedinoteApp, MAX_APPS>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::counter::{self, CounterState};
    // `hourglass` here names Medinote's own `apps::hourglass` (registration
    // glue, used below as `hourglass::descriptor::...`), not the `hourglass`
    // crate -- `HourglassModel`/`SessionState` are imported from the crate
    // explicitly via a leading `::` so the two never collide.
    use crate::apps::hourglass;
    use ::hourglass::model::{HourglassModel, SessionState};
    use shell::types::{ProviderGeneration, ProviderToken, SurfaceRef};

    /// Stands in for what a real target's `SurfaceRegistry::register_provider`
    /// hands back, without needing a full registry roundtrip just to prove
    /// resolution distinguishes two real apps.
    fn token(provider: shell::types::ProviderId) -> ProviderToken {
        ProviderToken {
            id: provider,
            generation: ProviderGeneration(1),
        }
    }

    /// Both of Medinote's actual product apps, from their own descriptors --
    /// not fabricated stand-ins -- registered the same way a real target
    /// would after registering each app's provider.
    fn real_apps() -> MedinoteApps {
        let mut apps = MedinoteApps::new();
        apps.register(MedinoteRegistration::new(
            &hourglass::descriptor::DESCRIPTOR,
            token(hourglass::descriptor::PROVIDER_ID),
        ))
        .expect("hourglass registers");
        apps.register(MedinoteRegistration::new(
            &counter::descriptor::DESCRIPTOR,
            token(counter::descriptor::PROVIDER_ID),
        ))
        .expect("counter registers");
        apps
    }

    fn launch_surface(entry: &MedinoteRegisteredApp) -> SurfaceRef {
        entry.launch_surface()
    }

    /// The core isolation property this plan's "production-path second app
    /// fixture" exists to prove, against the two apps Medinote actually
    /// compiles: resolving each real app's own launch surface never confuses
    /// one for the other, and an unrelated surface (Home's) resolves to
    /// neither.
    #[test]
    fn app_for_surface_distinguishes_the_two_real_apps() {
        let apps = real_apps();
        let home = SurfaceRef {
            owner: token(crate::catalogue::BASE_PROVIDER_ID),
            id: crate::catalogue::HOME_SURFACE_ID,
        };

        assert_eq!(
            apps.app_for_surface(launch_surface(&apps.as_slice()[0])),
            Some(MedinoteApp::Hourglass)
        );
        assert_eq!(
            apps.app_for_surface(launch_surface(&apps.as_slice()[1])),
            Some(MedinoteApp::Counter)
        );
        assert_eq!(apps.app_for_surface(home), None);
    }

    /// Launch lookups stay with their kind: each real app's kind resolves
    /// back to the surface its own descriptor registered.
    #[test]
    fn launch_surface_for_returns_each_real_app_surface() {
        let apps = real_apps();
        assert_eq!(
            apps.launch_surface_for(MedinoteApp::Hourglass),
            Some(launch_surface(&apps.as_slice()[0]))
        );
        assert_eq!(
            apps.launch_surface_for(MedinoteApp::Counter),
            Some(launch_surface(&apps.as_slice()[1]))
        );
    }

    /// A KEY-shaped input event, addressed by destination surface exactly the
    /// way the target's runtime loop resolves the active app: resolve a
    /// [`MedinoteApp`] through `app_for_surface`, then touch that app's own
    /// real state only.
    fn deliver_key(
        apps: &MedinoteApps,
        destination: SurfaceRef,
        hourglass_model: &mut HourglassModel,
        counter_state: &mut CounterState,
    ) {
        match apps
            .app_for_surface(destination)
            .expect("test destinations always resolve")
        {
            MedinoteApp::Hourglass => {
                hourglass_model.start();
            }
            MedinoteApp::Counter => {
                counter_state.increment();
            }
            #[cfg(feature = "network-controls")]
            MedinoteApp::Network => {}
        }
    }

    /// The `Done when` proof, against the two apps' own real state types
    /// instead of a fake stand-in: dispatching a KEY-shaped input to
    /// whichever real app a destination surface resolves to only ever touches
    /// that app's own state -- the other's is untouched -- and ticking the
    /// active app's physics never advances the inactive one's. Counter has no
    /// tick cadence of its own at all (unlike Hourglass), which is itself
    /// part of the proof: the kind enum's own doc says app runtimes own their
    /// state/input/timing without being forced through one shared shape, and
    /// these two real apps genuinely differ that way.
    #[test]
    fn two_real_apps_each_receive_only_their_own_input_and_timing() {
        let apps = real_apps();
        let mut hourglass_model = HourglassModel::new();
        let mut counter_state = CounterState::new();
        assert_eq!(hourglass_model.state(), SessionState::Ready);
        assert_eq!(counter_state.count(), 0);

        // KEY addressed to Counter's launch surface must never reach
        // Hourglass's model.
        deliver_key(
            &apps,
            launch_surface(&apps.as_slice()[1]),
            &mut hourglass_model,
            &mut counter_state,
        );
        assert_eq!(counter_state.count(), 1);
        assert_eq!(
            hourglass_model.state(),
            SessionState::Ready,
            "Counter's input must never reach Hourglass's model"
        );

        // KEY addressed to Hourglass's launch surface must actually reach it,
        // and must never advance Counter's count.
        deliver_key(
            &apps,
            launch_surface(&apps.as_slice()[0]),
            &mut hourglass_model,
            &mut counter_state,
        );
        assert_ne!(
            hourglass_model.state(),
            SessionState::Ready,
            "Hourglass's own input must actually reach it"
        );
        assert_eq!(
            counter_state.count(),
            1,
            "Hourglass's input must never reach Counter's state"
        );

        // Timing: ticking Hourglass's physics must never advance Counter.
        let before = hourglass_model.positions().fold(0u64, |hash, position| {
            hash.wrapping_mul(0x100_0000_01B3)
                ^ position.x.raw() as u32 as u64
                ^ (position.y.raw() as u32 as u64).rotate_left(32)
        });
        for _ in 0..30 {
            hourglass_model.tick();
        }
        let after = hourglass_model.positions().fold(0u64, |hash, position| {
            hash.wrapping_mul(0x100_0000_01B3)
                ^ position.x.raw() as u32 as u64
                ^ (position.y.raw() as u32 as u64).rotate_left(32)
        });
        assert_ne!(
            after, before,
            "gravity must change the occupied cells after 30 real physics ticks"
        );
        assert_eq!(
            counter_state.count(),
            1,
            "ticking Hourglass must never advance Counter's count"
        );
    }
}
