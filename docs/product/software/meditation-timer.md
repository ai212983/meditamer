---
author: dimitri
---
# Meditation Timer

Meditation Timer is implemented as a filling Enso circle, which is a visual indicator of the session progress.
This timer can be cancelled or paused, and the user can choose to set a timer for a specific duration.
While meditation is intended use case, the timer can be used for any activity.

## Usage
Starting screen contains:
- Time duration or target time selector
  - Time duration selector contains hours, minutes and seconds
- Last 5 used durations for quick selection

Once started, the screen shows filling Enso circle. 
No remaining hours, minutes or seconds is shown to avoid distraction.

When screen is tapped in central area, an overlay is shown in the center.
Overlay contains:
 - Remaining time in minutes and seconds. Seconds are ticking, if timer is not paused.
 - Button to "cancel" the timer (it will return to the home screen)
 - Button to "pause" or "resume" the timer

 If timer is "paused", there's no option to return to the timer screen
 If timer is "running", tapping outside the overlay will return to the timer screen.

 Overlay is opened with partial screen update, closed with full update.

 When timer is finished, the screen turns on backlight and beep sound is played. 
 Backlight is fading out after 3 seconds.
