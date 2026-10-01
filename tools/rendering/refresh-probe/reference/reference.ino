#include "Inkplate.h"
#include "probe_config.h"

#include <Esp.h>

Inkplate display(INKPLATE_1BIT);

using namespace RefreshProbeConfig;

constexpr uint32_t kColdPartialSamples = kReferenceColdPartialSamples;
#if defined(REFRESH_PROBE_PROFILE_PARTIAL_1BIT_ONLY)
constexpr uint32_t kWarmPartialSamples = kPartialProfileSamples;
constexpr const char *kProbeMode = "partial_1bit_profile";
#elif defined(REFRESH_PROBE_PARTIAL_1BIT_ONLY)
constexpr uint32_t kWarmPartialSamples = kPartialSamples;
constexpr const char *kProbeMode = "partial_1bit_performance";
#else
constexpr uint32_t kWarmPartialSamples = kPartialSamples;
#if defined(REFRESH_PROBE_PROFILE_FULL_1BIT_ONLY)
constexpr uint32_t kRunColdPartialSamples = 0;
constexpr const char *kProbeMode = "full_1bit_profile";
#elif defined(REFRESH_PROBE_PROFILE_FULL_3BIT_ONLY)
constexpr uint32_t kRunColdPartialSamples = 0;
constexpr const char *kProbeMode = "full_3bit_profile";
#elif defined(REFRESH_PROBE_FULL_1BIT_ONLY)
constexpr uint32_t kRunColdPartialSamples = 0;
constexpr const char *kProbeMode = "full_1bit_performance";
#elif defined(REFRESH_PROBE_FULL_3BIT_ONLY)
constexpr uint32_t kRunColdPartialSamples = 0;
constexpr const char *kProbeMode = "full_3bit_performance";
#else
constexpr uint32_t kRunColdPartialSamples = kColdPartialSamples;
constexpr const char *kProbeMode = "all";
#endif
#endif
#if defined(REFRESH_PROBE_PARTIAL_1BIT_ONLY) || \
    defined(REFRESH_PROBE_PROFILE_PARTIAL_1BIT_ONLY)
constexpr uint32_t kRunColdPartialSamples = 0;
#endif
constexpr uint32_t kGrayPartialContractSamples = kReferenceGrayPartialContractSamples;
constexpr uint32_t kGrayFullSamples = kReferenceGrayFullSamples;
constexpr size_t kGrayRowBytes = E_INK_WIDTH / 2;
constexpr size_t kGrayFramebufferBytes = E_INK_WIDTH * E_INK_HEIGHT / 2;

#if defined(REFRESH_PROBE_PROFILE_FULL_1BIT_ONLY) || \
    defined(REFRESH_PROBE_PROFILE_FULL_3BIT_ONLY)
constexpr uint32_t kFullRunSamples = kFullProfileSamples;
constexpr const char *kFullPurpose = "phase_profile";
constexpr const char *kFullInstrumentation = "phase_timestamps";
#else
constexpr uint32_t kFullRunSamples = kFullPerformanceSamples;
constexpr const char *kFullPurpose = "performance";
constexpr const char *kFullInstrumentation = "none";
#endif

#if defined(REFRESH_PROBE_PROFILE_PARTIAL_1BIT_ONLY)
constexpr const char *kPartialPurpose = "phase_profile";
constexpr const char *kPartialInstrumentation = "phase_timestamps";
#else
constexpr const char *kPartialPurpose = "performance";
constexpr const char *kPartialInstrumentation = "none";
#endif

struct Timing {
    uint32_t elapsedUs;
    uint32_t elapsedCycles;
};

uint32_t previousRefreshFinishedUs = 0;

Timing elapsedSince(uint32_t startedUs, uint32_t startedCycles)
{
    return {micros() - startedUs, ESP.getCycleCount() - startedCycles};
}

void printTiming(const char *event, const char *phase, uint32_t sample, uint32_t marker,
                 const char *pattern, const Timing &timing)
{
    Serial.printf(
        "REFERENCE_REFRESH event=%s phase=%s sample=%lu marker=%03lu pattern=%s "
        "elapsed_us=%lu elapsed_cycles=%lu visual=unverified\n",
        event, phase, static_cast<unsigned long>(sample), static_cast<unsigned long>(marker),
        pattern, static_cast<unsigned long>(timing.elapsedUs),
        static_cast<unsigned long>(timing.elapsedCycles));
}

void drawMarker(uint32_t marker)
{
    const uint32_t digitAdvance = (kDigitWidth + 1) * kMarkerScale;
    const uint32_t contentWidth = kMarkerDigits * digitAdvance - kMarkerScale;
    const uint32_t contentHeight = kDigitHeight * kMarkerScale;
    const uint32_t badgeWidth = contentWidth + kMarkerPadding * 2;
    const uint32_t badgeHeight = contentHeight + kMarkerPadding * 2;
    const uint32_t badgeX = (kWidth - badgeWidth) / 2;
    const uint32_t badgeY = (kHeight - badgeHeight) / 2;

    display.fillRect(badgeX, badgeY, badgeWidth, badgeHeight, WHITE);
    display.drawRect(badgeX, badgeY, badgeWidth, badgeHeight, BLACK);

    marker %= 1000;
    const uint8_t digits[] = {
        static_cast<uint8_t>(marker / 100),
        static_cast<uint8_t>((marker / 10) % 10),
        static_cast<uint8_t>(marker % 10),
    };

    for (uint32_t digitIndex = 0; digitIndex < kMarkerDigits; ++digitIndex)
    {
        const uint32_t originX = badgeX + kMarkerPadding + digitIndex * digitAdvance;
        const uint32_t originY = badgeY + kMarkerPadding;
        for (uint32_t row = 0; row < kDigitHeight; ++row)
        {
            const uint8_t bits = kDigitRows[digits[digitIndex]][row];
            for (uint32_t column = 0; column < kDigitWidth; ++column)
            {
                if ((bits & (1U << (kDigitWidth - 1 - column))) == 0)
                    continue;
                display.fillRect(originX + column * kMarkerScale,
                                 originY + row * kMarkerScale,
                                 kMarkerScale, kMarkerScale, BLACK);
            }
        }
    }
}

const char *drawPattern(uint32_t marker)
{
    display.clearDisplay();
    const bool vertical = (marker & 1U) == 0;
    if (vertical)
    {
        for (uint32_t x = 0; x < kWidth; x += kBandWidth * 2)
            display.fillRect(x, 0, kBandWidth, kHeight, BLACK);
    }
    else
    {
        for (uint32_t y = 0; y < kHeight; y += kBandWidth * 2)
            display.fillRect(0, y, kWidth, kBandWidth, BLACK);
    }
    drawMarker(marker);
    return vertical ? "vertical_50px" : "horizontal_50px";
}

void measureDraw(const char *phase, uint32_t sample, uint32_t marker, const char **pattern)
{
    const uint32_t startedUs = micros();
    const uint32_t startedCycles = ESP.getCycleCount();
    *pattern = drawPattern(marker);
    printTiming("draw", phase, sample, marker, *pattern, elapsedSince(startedUs, startedCycles));
}

void measureFull(const char *phase, uint32_t sample, uint32_t marker, const char *pattern,
                 bool leaveOn)
{
    const uint32_t startIntervalUs = previousRefreshFinishedUs == 0
                                         ? 0
                                         : micros() - previousRefreshFinishedUs;
    const uint32_t startedUs = micros();
    const uint32_t startedCycles = ESP.getCycleCount();
    display.display(leaveOn);
    const Timing timing = elapsedSince(startedUs, startedCycles);
    previousRefreshFinishedUs = micros();
    Serial.printf(
        "REFERENCE_REFRESH event=refresh phase=%s kind=full sample=%lu marker=%03lu pattern=%s "
        "leave_on=%u start_interval_us=%lu elapsed_us=%lu elapsed_cycles=%lu visual=unverified\n",
        phase, static_cast<unsigned long>(sample), static_cast<unsigned long>(marker), pattern,
        leaveOn ? 1U : 0U, static_cast<unsigned long>(startIntervalUs),
        static_cast<unsigned long>(timing.elapsedUs),
        static_cast<unsigned long>(timing.elapsedCycles));
#ifdef INKPLATE_REFRESH_PROBE_FULL_TIMING
    const InkplateFullUpdateTiming &phases = display.getLastFullUpdateTiming();
    const uint32_t waveformUs = phases.initialCleanUs + phases.framebufferUs +
                                phases.settleUs + phases.finalCleanUs +
                                phases.terminalVscanUs;
    Serial.printf(
        "REFERENCE_REFRESH event=phases phase=%s kind=full sample=%lu marker=%03lu "
        "preparation_us=%lu power_on_us=%lu waveform_us=%lu initial_clean_us=%lu "
        "framebuffer_us=%lu settle_us=%lu final_clean_us=%lu terminal_vscan_us=%lu "
        "finalization_us=%lu previous_copy_us=0 instrumented_total_us=%lu\n",
        phase, static_cast<unsigned long>(sample), static_cast<unsigned long>(marker),
        static_cast<unsigned long>(phases.preparationUs),
        static_cast<unsigned long>(phases.powerOnUs), static_cast<unsigned long>(waveformUs),
        static_cast<unsigned long>(phases.initialCleanUs),
        static_cast<unsigned long>(phases.framebufferUs),
        static_cast<unsigned long>(phases.settleUs),
        static_cast<unsigned long>(phases.finalCleanUs),
        static_cast<unsigned long>(phases.terminalVscanUs),
        static_cast<unsigned long>(phases.powerOffUs),
        static_cast<unsigned long>(phases.totalUs));
#endif
}

void measurePartial(const char *phase, uint32_t sample, uint32_t marker, const char *pattern,
                    bool leaveOn)
{
    const uint32_t startIntervalUs = previousRefreshFinishedUs == 0
                                         ? 0
                                         : micros() - previousRefreshFinishedUs;
    const uint32_t startedUs = micros();
    const uint32_t startedCycles = ESP.getCycleCount();
    const uint32_t blackToWhite = display.partialUpdate(true, leaveOn);
    const Timing timing = elapsedSince(startedUs, startedCycles);
    previousRefreshFinishedUs = micros();
#ifdef INKPLATE_REFRESH_PROBE_TIMING
    const InkplatePartialUpdateTiming &phases = display.getLastPartialUpdateTiming();
#endif
    Serial.printf(
        "REFERENCE_REFRESH event=refresh phase=%s kind=partial sample=%lu marker=%03lu pattern=%s "
        "forced=1 leave_on=%u black_to_white_pixels=%lu start_interval_us=%lu "
        "elapsed_us=%lu elapsed_cycles=%lu "
        "visual=unverified\n",
        phase, static_cast<unsigned long>(sample), static_cast<unsigned long>(marker), pattern,
        leaveOn ? 1U : 0U, static_cast<unsigned long>(blackToWhite),
        static_cast<unsigned long>(startIntervalUs),
        static_cast<unsigned long>(timing.elapsedUs),
        static_cast<unsigned long>(timing.elapsedCycles));
#ifdef INKPLATE_REFRESH_PROBE_TIMING
    Serial.printf(
        "REFERENCE_REFRESH event=phases phase=%s kind=partial sample=%lu marker=%03lu "
        "preparation_us=%lu power_on_us=%lu transition_scan_us=%lu cleanup_us=%lu "
        "finalization_us=%lu power_off_us=%lu previous_copy_us=%lu "
        "instrumented_total_us=%lu\n",
        phase, static_cast<unsigned long>(sample), static_cast<unsigned long>(marker),
        static_cast<unsigned long>(phases.preparationUs),
        static_cast<unsigned long>(phases.powerOnUs),
        static_cast<unsigned long>(phases.transitionScanUs),
        static_cast<unsigned long>(phases.cleanupUs),
        static_cast<unsigned long>(phases.powerOffUs),
        static_cast<unsigned long>(phases.powerOffUs),
        static_cast<unsigned long>(phases.previousCopyUs),
        static_cast<unsigned long>(phases.totalUs));
#endif
}

void waitForRefreshInterval()
{
    if (previousRefreshFinishedUs == 0)
        return;
    const uint32_t elapsedUs = micros() - previousRefreshFinishedUs;
    const uint32_t targetUs = kRefreshIntervalMs * 1000U;
    if (elapsedUs < targetUs)
        delayMicroseconds(targetUs - elapsedUs);
}

void measurePowerOn(const char *phase)
{
    const uint32_t startedUs = micros();
    const uint32_t startedCycles = ESP.getCycleCount();
    const int result = display.einkOn();
    const Timing timing = elapsedSince(startedUs, startedCycles);
    const double panelVcom = result == 1 ? display.getStoredVCOM() : NAN;
    Serial.printf(
        "REFERENCE_REFRESH event=power phase=%s action=on result=%d power_good=%u panel_vcom_v=%.2f "
        "elapsed_us=%lu elapsed_cycles=%lu\n",
        phase, result, display.isPowerGood() ? 1U : 0U, panelVcom,
        static_cast<unsigned long>(timing.elapsedUs),
        static_cast<unsigned long>(timing.elapsedCycles));
}

void measurePowerOff(const char *phase)
{
    const uint32_t startedUs = micros();
    const uint32_t startedCycles = ESP.getCycleCount();
    display.einkOff();
    const Timing timing = elapsedSince(startedUs, startedCycles);
    Serial.printf(
        "REFERENCE_REFRESH event=power phase=%s action=off elapsed_us=%lu elapsed_cycles=%lu\n",
        phase, static_cast<unsigned long>(timing.elapsedUs),
        static_cast<unsigned long>(timing.elapsedCycles));
}

uint8_t gray3At(size_t x, size_t y)
{
    if (y < kGrayBandHeight)
    {
        const size_t physicalLevel = min(x * 8 / E_INK_WIDTH, size_t(7));
        return static_cast<uint8_t>(physicalLevel * 2);
    }
    if (y < kGrayBandHeight * 2)
        return static_cast<uint8_t>(x * 15 / (E_INK_WIDTH - 1));

    const size_t checker =
        ((x / kGrayCheckerTile) + ((y - kGrayBandHeight * 2) / kGrayCheckerTile)) & 1U;
    if (x < E_INK_WIDTH / 2)
        return checker == 0 ? 0 : 15;
    return checker == 0 ? 6 : 8;
}

void fillGray3ReferencePattern(uint8_t *framebuffer)
{
    for (size_t index = 0; index < kGrayFramebufferBytes; ++index)
    {
        const size_t y = index / kGrayRowBytes;
        const size_t x = (index % kGrayRowBytes) * 2;
        // The panel presents packed framebuffer coordinates 90 degrees
        // counterclockwise. Store the logical fixture rotated clockwise so the
        // ramps, checkerboards, and marker are upright on the physical screen.
        framebuffer[index] = static_cast<uint8_t>(
            (gray3At(y, E_INK_HEIGHT - 1 - x) << 4) |
            gray3At(y, E_INK_HEIGHT - 2 - x));
    }
}

void setGray4Pixel(uint8_t *framebuffer, size_t x, size_t y, uint8_t level)
{
    const size_t framebufferX = E_INK_HEIGHT - 1 - y;
    const size_t framebufferY = x;
    const size_t index = framebufferY * kGrayRowBytes + framebufferX / 2;
    if ((framebufferX & 1U) == 0)
        framebuffer[index] = static_cast<uint8_t>((framebuffer[index] & 0x0fU) | (level << 4));
    else
        framebuffer[index] = static_cast<uint8_t>((framebuffer[index] & 0xf0U) | level);
}

void fillGray4Rect(uint8_t *framebuffer, size_t x, size_t y, size_t width, size_t height,
                   uint8_t level)
{
    for (size_t row = y; row < y + height; ++row)
        for (size_t column = x; column < x + width; ++column)
            setGray4Pixel(framebuffer, column, row, level);
}

void drawGray3Marker(uint8_t *framebuffer, uint32_t marker)
{
    const size_t digitAdvance = (kDigitWidth + 1) * kMarkerScale;
    const size_t contentWidth = kMarkerDigits * digitAdvance - kMarkerScale;
    const size_t contentHeight = kDigitHeight * kMarkerScale;
    const size_t badgeWidth = contentWidth + kMarkerPadding * 2;
    const size_t badgeHeight = contentHeight + kMarkerPadding * 2;
    const size_t badgeX = (kWidth - badgeWidth) / 2;
    const size_t badgeY = (kHeight - badgeHeight) / 2;

    fillGray4Rect(framebuffer, badgeX, badgeY, badgeWidth, badgeHeight, 15);
    fillGray4Rect(framebuffer, badgeX, badgeY, badgeWidth, 1, 0);
    fillGray4Rect(framebuffer, badgeX, badgeY + badgeHeight - 1, badgeWidth, 1, 0);
    fillGray4Rect(framebuffer, badgeX, badgeY, 1, badgeHeight, 0);
    fillGray4Rect(framebuffer, badgeX + badgeWidth - 1, badgeY, 1, badgeHeight, 0);

    marker %= 1000;
    const uint8_t digits[] = {
        static_cast<uint8_t>(marker / 100),
        static_cast<uint8_t>((marker / 10) % 10),
        static_cast<uint8_t>(marker % 10),
    };
    for (size_t digitIndex = 0; digitIndex < kMarkerDigits; ++digitIndex)
    {
        const size_t originX = badgeX + kMarkerPadding + digitIndex * digitAdvance;
        const size_t originY = badgeY + kMarkerPadding;
        for (size_t row = 0; row < kDigitHeight; ++row)
        {
            const uint8_t bits = kDigitRows[digits[digitIndex]][row];
            for (size_t column = 0; column < kDigitWidth; ++column)
            {
                if ((bits & (1U << (kDigitWidth - 1 - column))) != 0)
                    fillGray4Rect(framebuffer, originX + column * kMarkerScale,
                                  originY + row * kMarkerScale, kMarkerScale,
                                  kMarkerScale, 0);
            }
        }
    }
}

uint32_t fnv1a(const uint8_t *bytes, size_t length)
{
    uint32_t hash = 0x811c9dc5;
    for (size_t index = 0; index < length; ++index)
        hash = (hash ^ bytes[index]) * 0x01000193;
    return hash;
}

uint32_t prepareNumberedGray3Pattern(const char *phase, uint32_t sample, uint32_t marker)
{
    display.selectDisplayMode(INKPLATE_3BIT);
    const uint32_t drawStartedUs = micros();
    const uint32_t drawStartedCycles = ESP.getCycleCount();
    fillGray3ReferencePattern(display.DMemory4Bit);
    const uint32_t baseHash = fnv1a(display.DMemory4Bit, kGrayFramebufferBytes);
    if (baseHash != kGrayExpectedFnv1A)
    {
        Serial.printf(
            "REFERENCE_REFRESH event=halt phase=%s sample=%lu stage=base_pattern_hash "
            "actual=0x%08lx expected=0x%08lx\n",
            phase, static_cast<unsigned long>(sample), static_cast<unsigned long>(baseHash),
            static_cast<unsigned long>(kGrayExpectedFnv1A));
        return 0;
    }
    drawGray3Marker(display.DMemory4Bit, marker);
    const uint32_t patternHash = fnv1a(display.DMemory4Bit, kGrayFramebufferBytes);
    const Timing draw = elapsedSince(drawStartedUs, drawStartedCycles);
    Serial.printf(
        "REFERENCE_REFRESH event=draw phase=%s display_mode=3bit sample=%lu marker=%03lu "
        "framebuffer_bytes=%lu pattern=levels_ramp_boundaries_numbered "
        "base_pattern_hash=0x%08lx pattern_hash=0x%08lx elapsed_us=%lu "
        "elapsed_cycles=%lu timed=0\n",
        phase, static_cast<unsigned long>(sample), static_cast<unsigned long>(marker),
        static_cast<unsigned long>(kGrayFramebufferBytes), static_cast<unsigned long>(baseHash),
        static_cast<unsigned long>(patternHash), static_cast<unsigned long>(draw.elapsedUs),
        static_cast<unsigned long>(draw.elapsedCycles));
    return patternHash;
}

void measureGrayPartialContract(uint32_t sample, uint32_t patternHash)
{
    const uint32_t startedUs = micros();
    const uint32_t startedCycles = ESP.getCycleCount();
    const uint32_t blackToWhite = display.partialUpdate(true, true);
    const Timing timing = elapsedSince(startedUs, startedCycles);
    Serial.printf(
        "REFERENCE_REFRESH event=refresh phase=gray3_partial_contract kind=partial "
        "display_mode=3bit support=unsupported_noop sample=%lu forced=1 leave_on=1 "
        "black_to_white_pixels=%lu panel_power_expected=off "
        "pattern_hash=0x%08lx elapsed_us=%lu elapsed_cycles=%lu visual=not_applicable\n",
        static_cast<unsigned long>(sample), static_cast<unsigned long>(blackToWhite),
        static_cast<unsigned long>(patternHash),
        static_cast<unsigned long>(timing.elapsedUs),
        static_cast<unsigned long>(timing.elapsedCycles));
}

void measureGrayFull(const char *phase, uint32_t sample, uint32_t patternHash)
{
    waitForRefreshInterval();
    const uint32_t startIntervalUs = previousRefreshFinishedUs == 0
                                         ? 0
                                         : micros() - previousRefreshFinishedUs;
    const uint32_t totalStartedUs = micros();
    const uint32_t totalStartedCycles = ESP.getCycleCount();

    const uint32_t powerOnStartedUs = micros();
    const uint32_t powerOnStartedCycles = ESP.getCycleCount();
    const int powerOnResult = display.einkOn();
    const Timing powerOn = elapsedSince(powerOnStartedUs, powerOnStartedCycles);
    if (powerOnResult != 1)
    {
        Serial.printf(
            "REFERENCE_REFRESH event=halt phase=%s sample=%lu stage=power_on "
            "result=%d elapsed_us=%lu\n",
            phase, static_cast<unsigned long>(sample), powerOnResult,
            static_cast<unsigned long>(powerOn.elapsedUs));
        return;
    }

    const uint32_t waveformStartedUs = micros();
    const uint32_t waveformStartedCycles = ESP.getCycleCount();
    display.display(true);
    const Timing waveform = elapsedSince(waveformStartedUs, waveformStartedCycles);

    const uint32_t powerOffStartedUs = micros();
    const uint32_t powerOffStartedCycles = ESP.getCycleCount();
    display.einkOff();
    const Timing powerOff = elapsedSince(powerOffStartedUs, powerOffStartedCycles);
    const Timing total = elapsedSince(totalStartedUs, totalStartedCycles);
    previousRefreshFinishedUs = micros();

    Serial.printf(
        "REFERENCE_REFRESH event=refresh phase=%s kind=full display_mode=3bit "
        "sample=%lu pattern=levels_ramp_boundaries pattern_hash=0x%08lx passes=74 rows=44400 "
        "cl_pulses=6704400 start_interval_us=%lu power_on_us=%lu waveform_us=%lu "
        "power_off_us=%lu total_us=%lu total_cycles=%lu visual=unverified\n",
        phase, static_cast<unsigned long>(sample), static_cast<unsigned long>(patternHash),
        static_cast<unsigned long>(startIntervalUs),
        static_cast<unsigned long>(powerOn.elapsedUs),
        static_cast<unsigned long>(waveform.elapsedUs),
        static_cast<unsigned long>(powerOff.elapsedUs),
        static_cast<unsigned long>(total.elapsedUs),
        static_cast<unsigned long>(total.elapsedCycles));
}

void measureGrayFullTransaction(const char *phase, uint32_t sample, uint32_t marker,
                                uint32_t patternHash)
{
    waitForRefreshInterval();
    const uint32_t startIntervalUs = previousRefreshFinishedUs == 0
                                         ? 0
                                         : micros() - previousRefreshFinishedUs;
    const uint32_t startedUs = micros();
    const uint32_t startedCycles = ESP.getCycleCount();
    display.display(false);
    const Timing timing = elapsedSince(startedUs, startedCycles);
    previousRefreshFinishedUs = micros();
    Serial.printf(
        "REFERENCE_REFRESH event=refresh phase=%s kind=full display_mode=3bit "
        "sample=%lu marker=%03lu pattern=levels_ramp_boundaries_numbered pattern_hash=0x%08lx "
        "start_interval_us=%lu elapsed_us=%lu elapsed_cycles=%lu leave_on=0 "
        "visual=unverified\n",
        phase, static_cast<unsigned long>(sample), static_cast<unsigned long>(marker),
        static_cast<unsigned long>(patternHash),
        static_cast<unsigned long>(startIntervalUs),
        static_cast<unsigned long>(timing.elapsedUs),
        static_cast<unsigned long>(timing.elapsedCycles));
#ifdef INKPLATE_REFRESH_PROBE_FULL_TIMING
    const InkplateFullUpdateTiming &phases = display.getLastFullUpdateTiming();
    const uint32_t waveformUs = phases.initialCleanUs + phases.framebufferUs +
                                phases.settleUs + phases.finalCleanUs +
                                phases.terminalVscanUs;
    Serial.printf(
        "REFERENCE_REFRESH event=phases phase=%s kind=full display_mode=3bit sample=%lu "
        "marker=%03lu pattern_hash=0x%08lx preparation_us=%lu power_on_us=%lu waveform_us=%lu "
        "initial_clean_us=%lu framebuffer_us=%lu settle_us=%lu final_clean_us=%lu "
        "terminal_vscan_us=%lu finalization_us=%lu previous_copy_us=0 "
        "instrumented_total_us=%lu\n",
        phase, static_cast<unsigned long>(sample), static_cast<unsigned long>(marker),
        static_cast<unsigned long>(patternHash),
        static_cast<unsigned long>(phases.preparationUs),
        static_cast<unsigned long>(phases.powerOnUs), static_cast<unsigned long>(waveformUs),
        static_cast<unsigned long>(phases.initialCleanUs),
        static_cast<unsigned long>(phases.framebufferUs),
        static_cast<unsigned long>(phases.settleUs),
        static_cast<unsigned long>(phases.finalCleanUs),
        static_cast<unsigned long>(phases.terminalVscanUs),
        static_cast<unsigned long>(phases.powerOffUs),
        static_cast<unsigned long>(phases.totalUs));
#endif
}

uint32_t prepareGray3Pattern()
{
    display.selectDisplayMode(INKPLATE_3BIT);
    const uint32_t drawStartedUs = micros();
    const uint32_t drawStartedCycles = ESP.getCycleCount();
    fillGray3ReferencePattern(display.DMemory4Bit);
    const Timing draw = elapsedSince(drawStartedUs, drawStartedCycles);
    const uint32_t patternHash = fnv1a(display.DMemory4Bit, kGrayFramebufferBytes);
    Serial.printf(
        "REFERENCE_REFRESH event=draw phase=gray3_reference display_mode=3bit "
        "framebuffer_bytes=%lu pattern=levels_ramp_boundaries pattern_hash=0x%08lx "
        "expected_hash=0x%08lx elapsed_us=%lu elapsed_cycles=%lu timed=0\n",
        static_cast<unsigned long>(kGrayFramebufferBytes),
        static_cast<unsigned long>(patternHash),
        static_cast<unsigned long>(kGrayExpectedFnv1A),
        static_cast<unsigned long>(draw.elapsedUs),
        static_cast<unsigned long>(draw.elapsedCycles));
    return patternHash;
}

void setup()
{
    Serial.begin(115200);
    delay(1000);
    Serial.printf(
        "REFERENCE_REFRESH event=boot probe=reference-refresh mode=%s spec_version=%lu library=11.1.4 "
        "fqbn=Inkplate_Boards:esp32:Inkplate4TEMPERA cpu_hz=%lu cold_samples=%lu "
        "warm_samples=%lu partial_profile_samples=%lu full_performance_samples=%lu "
        "full_profile_samples=%lu "
        "gray_partial_contract_samples=%lu gray_full_samples=%lu "
        "minimum_refresh_end_to_start_interval_ms=%lu pattern_bands_px=50 "
        "marker=rust_fixture_equivalent visual=unverified\n",
        kProbeMode, static_cast<unsigned long>(kSpecVersion), static_cast<unsigned long>(kCpuHz),
        static_cast<unsigned long>(kRunColdPartialSamples),
        static_cast<unsigned long>(kWarmPartialSamples),
        static_cast<unsigned long>(kPartialProfileSamples),
        static_cast<unsigned long>(kFullPerformanceSamples),
        static_cast<unsigned long>(kFullProfileSamples),
        static_cast<unsigned long>(kGrayPartialContractSamples),
        static_cast<unsigned long>(kGrayFullSamples),
        static_cast<unsigned long>(kRefreshIntervalMs));

    display.begin();
    display.setFullUpdateThreshold(0);
    Serial.printf(
        "REFERENCE_REFRESH event=environment phase=before panel_temperature_c=%d "
        "configured_vcom_v=%.2f\n",
        display.readTemperature(), display.getVCOMValue());

#if defined(REFRESH_PROBE_FULL_3BIT_ONLY) || \
    defined(REFRESH_PROBE_PROFILE_FULL_3BIT_ONLY)
    {
        uint32_t grayPatternHash = prepareNumberedGray3Pattern("baseline", 0, 0);
        if (grayPatternHash == 0)
            return;
        measureGrayFullTransaction("baseline", 0, 0, grayPatternHash);
        Serial.printf(
            "REFERENCE_REFRESH event=benchmark_start benchmark=full_3bit purpose=%s "
            "samples=%lu warmup_samples=1 power_state=cold_per_sample "
            "refresh_end_to_start_interval_ms=%lu draw_timed=0 api=display_false "
            "fixture=levels_ramp_boundaries_numbered marker_scheme=mod1000 "
            "instrumentation=%s\n",
            kFullPurpose, static_cast<unsigned long>(kFullRunSamples),
            static_cast<unsigned long>(kRefreshIntervalMs), kFullInstrumentation);
        for (uint32_t sample = 1; sample <= kFullRunSamples; ++sample)
        {
            const uint32_t marker = sample % 1000;
            grayPatternHash = prepareNumberedGray3Pattern(kProbeMode, sample, marker);
            if (grayPatternHash == 0)
                return;
            measureGrayFullTransaction(kProbeMode, sample, marker, grayPatternHash);
        }
        Serial.printf(
            "REFERENCE_REFRESH event=environment phase=after panel_temperature_c=%d "
            "configured_vcom_v=%.2f\n",
            display.readTemperature(), display.getVCOMValue());
        Serial.printf(
            "REFERENCE_REFRESH event=complete protocol=passed mode=%s "
            "purpose=%s benchmark=full_3bit samples=%lu warmup_samples=1 "
            "final_marker=%03lu final_pattern=levels_ramp_boundaries_numbered "
            "final_pattern_hash=0x%08lx "
            "screen_updates=stopped visual=unverified\n",
            kProbeMode, kFullPurpose, static_cast<unsigned long>(kFullRunSamples),
            static_cast<unsigned long>(kFullRunSamples % 1000),
            static_cast<unsigned long>(grayPatternHash));
        return;
    }
#endif

    const char *pattern = nullptr;
#if defined(REFRESH_PROBE_PARTIAL_1BIT_ONLY) || \
    defined(REFRESH_PROBE_PROFILE_PARTIAL_1BIT_ONLY)
    measureDraw("baseline", 0, 1, &pattern);
    measureFull("baseline", 0, 1, pattern, false);
#else
    measureDraw("baseline", 0, 0, &pattern);
    measureFull("baseline", 0, 0, pattern, false);
#endif

#if defined(REFRESH_PROBE_FULL_1BIT_ONLY) || \
    defined(REFRESH_PROBE_PROFILE_FULL_1BIT_ONLY)
    Serial.printf(
        "REFERENCE_REFRESH event=benchmark_start benchmark=full_1bit purpose=%s "
        "samples=%lu warmup_samples=1 power_state=cold_per_sample "
        "refresh_end_to_start_interval_ms=%lu draw_timed=0 api=display_false "
        "instrumentation=%s\n",
        kFullPurpose, static_cast<unsigned long>(kFullRunSamples),
        static_cast<unsigned long>(kRefreshIntervalMs), kFullInstrumentation);
    for (uint32_t sample = 1; sample <= kFullRunSamples; ++sample)
    {
        measureDraw(kProbeMode, sample, sample, &pattern);
        waitForRefreshInterval();
        measureFull(kProbeMode, sample, sample, pattern, false);
    }
    Serial.printf(
        "REFERENCE_REFRESH event=environment phase=after panel_temperature_c=%d "
        "configured_vcom_v=%.2f\n",
        display.readTemperature(), display.getVCOMValue());
    Serial.printf(
        "REFERENCE_REFRESH event=complete protocol=passed mode=%s "
        "purpose=%s benchmark=full_1bit samples=%lu warmup_samples=1 final_marker=%03lu "
        "final_mode=1bit screen_updates=stopped visual=unverified\n",
        kProbeMode, kFullPurpose, static_cast<unsigned long>(kFullRunSamples),
        static_cast<unsigned long>(kFullRunSamples));
    return;
#endif

#if !defined(REFRESH_PROBE_PARTIAL_1BIT_ONLY) && \
    !defined(REFRESH_PROBE_PROFILE_PARTIAL_1BIT_ONLY)
    measurePowerOn("isolated");
    measurePowerOff("isolated");
#endif

    for (uint32_t sample = 1; sample <= kRunColdPartialSamples; ++sample)
    {
        measureDraw("cold_partial", sample, sample, &pattern);
        waitForRefreshInterval();
        measurePartial("cold_partial", sample, sample, pattern, false);
    }

    measurePowerOn("warm_partial_setup");
#if defined(REFRESH_PROBE_PARTIAL_1BIT_ONLY) || \
    defined(REFRESH_PROBE_PROFILE_PARTIAL_1BIT_ONLY)
    measureDraw("baseline_partial", 0, 0, &pattern);
    waitForRefreshInterval();
    measurePartial("baseline", 0, 0, pattern, true);
    Serial.printf(
        "REFERENCE_REFRESH event=benchmark_start benchmark=partial_1bit purpose=%s "
        "samples=%lu warmup_samples=1 power_state=warm_held_on "
        "refresh_end_to_start_interval_ms=%lu draw_timed=0 api=partialUpdate_true_true "
        "instrumentation=%s\n",
        kPartialPurpose, static_cast<unsigned long>(kWarmPartialSamples),
        static_cast<unsigned long>(kRefreshIntervalMs), kPartialInstrumentation);
#endif
    for (uint32_t sample = 1; sample <= kWarmPartialSamples; ++sample)
    {
        const uint32_t marker = kRunColdPartialSamples + sample;
        measureDraw(kProbeMode, sample, marker, &pattern);
        waitForRefreshInterval();
        measurePartial(kProbeMode, sample, marker, pattern, true);
    }

#if defined(REFRESH_PROBE_PARTIAL_1BIT_ONLY) || \
    defined(REFRESH_PROBE_PROFILE_PARTIAL_1BIT_ONLY)
    measurePowerOff("partial_1bit_final");
    Serial.printf(
        "REFERENCE_REFRESH event=environment phase=after panel_temperature_c=%d "
        "configured_vcom_v=%.2f\n",
        display.readTemperature(), display.getVCOMValue());
    Serial.printf(
        "REFERENCE_REFRESH event=complete protocol=passed mode=%s purpose=%s "
        "benchmark=partial_1bit samples=%lu warmup_samples=1 final_marker=%03lu "
        "final_mode=1bit screen_updates=stopped "
        "visual=unverified\n",
        kProbeMode, kPartialPurpose,
        static_cast<unsigned long>(kWarmPartialSamples),
        static_cast<unsigned long>(kWarmPartialSamples));
    return;
#endif

    const uint32_t finalMarker = kRunColdPartialSamples + kWarmPartialSamples + 1;
    measureDraw("full_after_partial", 1, finalMarker, &pattern);
    waitForRefreshInterval();
    measureFull("full_after_partial", 1, finalMarker, pattern, false);

    const uint32_t grayPatternHash = prepareGray3Pattern();
    if (grayPatternHash != kGrayExpectedFnv1A)
    {
        Serial.printf("REFERENCE_REFRESH event=halt phase=gray3_reference stage=pattern_hash\n");
        return;
    }

    for (uint32_t sample = 1; sample <= kGrayPartialContractSamples; ++sample)
        measureGrayPartialContract(sample, grayPatternHash);

    for (uint32_t sample = 1; sample <= kGrayFullSamples; ++sample)
        measureGrayFull("gray3_full", sample, grayPatternHash);

    Serial.printf(
        "REFERENCE_REFRESH event=environment phase=after panel_temperature_c=%d "
        "configured_vcom_v=%.2f\n",
        display.readTemperature(), display.getVCOMValue());
    Serial.printf(
        "REFERENCE_REFRESH event=complete protocol=passed final_mode=3bit "
        "final_pattern=levels_ramp_boundaries final_pattern_hash=0x%08lx "
        "screen_updates=stopped visual=unverified\n",
        static_cast<unsigned long>(grayPatternHash));
}

void loop()
{
    delay(1000);
}
