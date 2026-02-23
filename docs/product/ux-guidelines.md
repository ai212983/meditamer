# Meditamer: E-Ink UX & Technical Guidelines

The hardware constraints of the Inkplate 4 TEMPERA directly shape the interaction model of Meditamer. Because e-ink is an inherently slow medium, the user interface must be designed to enforce calm, deliberate engagement.

These are the foundational technical and UX constraints for the MVP:

## 1. Core E-Ink UX Rules (The "Slowness" Enforcers)
The physical realities of the screen demand exactly the calm interactions outlined in our product vision.
* **No "Live" UI:** Even though partial refreshes are fast, smooth ticking countdowns or progress animations burn power rapidly and accumulate visual artifacts (ghosting) over time. Updates should still be coarse (e.g., every 5 minutes, or at 25/50/75% milestones) to preserve battery and maintain a calm environment.
* **The Touch Blackout & Partial Refresh:** Capacitive touch is temporarily disabled during a full screen refresh (~0.86s). Because of this, Meditamer's UI relies almost entirely on fast partial refreshes (~0.18s or less) for transitions and interactions to ensure the interface feels snappy and responsive. Full refreshes are isolated to cleanup routines when the user is not actively tapping.
* **Two-Step Disruptive Actions:** Ending a session or turning on Wi-Fi are high-stakes actions that break the "protective container" (and often trigger a full cleanup refresh or massive battery draw). These should require a confirm gesture (a press-and-hold, or a second tap) to prevent accidental disruption.

## 2. Display Rendering Policies
Because we rely heavily on partial refresh to maintain UI responsiveness, all screens must adhere to these rendering primitives:
* **Stable Screens (Default):** The Ambient Home Screen and the Anchor Schedule. Render once, stay static until a boundary is crossed or the user taps.
* **UI Interactions & Micro-Updates (Partial Refresh):** This is the primary rendering mode for all user interactions (button taps, navigating views) and tiny deltas (like moving the "now" dot). It is fast (~0.18s) and effectively bypasses the touch blackout problem.
* **Cleanup Cadence (Full Refresh):** Used *only* at natural breaks (e.g., the end of a session, or when entering deep sleep mode) to clear accumulated ghosting from partial updates. It is intentionally separated from active UI navigation to protect the UX.

## 3. Power Management as a Principle
To be a reliable fixture that blends into the environment for weeks at a time, the power budget must be guarded ruthlessly.
* **Aggressive Deep Sleep:** The device must sleep deeply between anchors, waking only on user input or scheduled milestones. The Inkplate's ~18 µA deep sleep is what makes the "always-on" illusion possible.
* **Frontlight is a Stealth Killer:** Frontlight must be off by default during the day, aggressively time-limited when active, and ideally ambient-reactive (using the built-in APDS-9960 sensor) to utilize the minimum necessary brightness at night.
* **Wi-Fi is a "Special Occasion":** Wi-Fi transmission causes massive power spikes. It must be strictly opt-in, user-initiated (e.g., a "sync schedule" button), and never run in the background.

## 4. Accessibility and Legibility
* **Contrast First:** Even in 3-bit grayscale, use high contrast. Avoid faint grays for essential information. Grayscale should be used for secondary ambiance, not core instruction.
* **Typography:** Use large text options and simple fonts with strong strokes to ensure readability from a distance (e.g., across an office desk or kitchen counter).

## 5. MVP Feature Implications
* **Ambient Home Screens:** Rather than displaying precise numerical time, the primary view should leverage the "Stable Screen" primitive to ground the user in the current phase of the day, completely avoiding the anxiety of ticking clocks.
* **Session Timers with Coarse Milestones:** For bounded activities, the UI must avoid ticking seconds or granular countdowns. Focus instead on coarse milestones to minimize repetitive partial refreshes and preserve a calm environment.
* **Offline-First Storage:** Using the microSD card for schedule templates and user preferences guarantees the device works immediately without needing an expensive, battery-killing Wi-Fi connection.
