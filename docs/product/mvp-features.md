# Meditamer: MVP Feature Outline

Based on the [Product Vision](product-vision.md) and the [UX Guidelines](ux-guidelines.md), the Minimum Viable Product (MVP) for Meditamer must focus on establishing the device as a robust, offline-first anchor for the user's day.

## Core Philosophical Requirements for MVP
1. **Grounding, Not Tracking:** The device must pull the user out of the digital grind toward human events, not act as a productivity manager.
2. **Stable by Default:** The UI must rely on fast partial refreshes (~0.18s) for interaction and reserve full refreshes (~0.86s) exclusively for background cleanups.
3. **Radical Reliability:** The device must easily last weeks on a single charge through aggressive deep sleep and offline-first operation.

## Proposed MVP Feature Modules

### 1. The Ambient Home Screen
The default state of the device when not actively engaged in a localized session.
* **Abstract Time Representation:** Rather than a digital clock (e.g., "14:32"), the screen represents the shape of the day (e.g., via a sun-arc, time-lit scenery, or a daily progress bar).
* **Anchor Display:** The next major "human event" (Lunch, Workout, Family Dinner) is highlighted relative to the current time, providing immediate grounding context (e.g., "It is midafternoon; Dinner is approaching").
* **Low-Frequency Updates:** The UI only updates its abstract representation in coarse intervals (e.g., every 5 to 15 minutes) using rapid partial refreshes.

### 2. Localized Containment Sessions
The mode used when stepping away for a specific, bounded activity.
* **Coarse Progress Indicators:** When starting a session (e.g., a 20-minute tea break or a 45-minute reading hour), the UI displays simple, coarse milestones (a segmented ring or textual phrases like "Just started," "Halfway," "Almost done") instead of a ticking countdown.
* **Intentional Controls:** Starting, pausing, or ending a session requires intentful interaction (e.g., a hardware button press or a deliberate confirmation tap) to prevent accidental disruption of the boundary.
* **Hardware-Agnostic Cues:** When a session concludes, the device uses a gentle, appropriate acoustic or visual cue (e.g., a simple chime or subtle screen flash) rather than aggressive buzzer melodies.

### 3. Offline-First Configuration
The system allowing the user to set up their anchors and lengths.
* **SD Card Configuration:** The device loads "anchor schedules" (e.g., Workday vs. Weekend) and session length presets directly from a local `.json` or `.csv` file upon boot.
* **No Mandatory App:** While a companion web UI or app could exist later, the MVP requires *zero* cloud connectivity or Bluetooth tethering to function on a daily basis.

---

*Note: This is a working draft. Specific UI paradigms and hardware button mappings will be defined in subsequent design phases.*
