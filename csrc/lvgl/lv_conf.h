#ifndef LV_CONF_H
#define LV_CONF_H

/* Preserve luminance until the Rust flush path performs deterministic
 * element-aware dithering into the Inkplate's monochrome framebuffer. */
#define LV_COLOR_DEPTH 8
#define LV_USE_OS LV_OS_NONE
#define LV_DEF_REFR_PERIOD 250
#define LV_DPI_DEF 100

/* Keep LVGL's TLSF heap in the board's external PSRAM. */
#define LV_MEM_SIZE (128U * 1024U)
#define LV_MEM_POOL_EXPAND_SIZE 0
#define LV_MEM_POOL_INCLUDE "lvgl_psram.h"
#define LV_MEM_POOL_ALLOC(size) meditamer_lvgl_alloc_pool(size)

/* Retain only the software formats needed by the L8 renderer. */
#define LV_DRAW_SW_SUPPORT_RGB565 0
#define LV_DRAW_SW_SUPPORT_RGB565_SWAPPED 0
#define LV_DRAW_SW_SUPPORT_RGB565A8 0
#define LV_DRAW_SW_SUPPORT_RGB888 0
#define LV_DRAW_SW_SUPPORT_XRGB8888 0
#define LV_DRAW_SW_SUPPORT_ARGB8888 0
#define LV_DRAW_SW_SUPPORT_ARGB8888_PREMULTIPLIED 0
#define LV_DRAW_SW_SUPPORT_L8 1
#define LV_DRAW_SW_SUPPORT_AL88 0
#define LV_DRAW_SW_SUPPORT_A8 0
#define LV_DRAW_SW_SUPPORT_I1 0
#define LV_DRAW_SW_COMPLEX 1
#define LV_USE_DRAW_SW_COMPLEX_GRADIENTS 0

/* E-paper UI should not schedule continuous animation work. */
#define LV_USE_LOG 0
#define LV_USE_ANIMIMG 0
#define LV_USE_LOTTIE 0
#define LV_USE_GIF 0
#define LV_THEME_DEFAULT_GROW 0
#define LV_THEME_DEFAULT_TRANSITION_TIME 0

/* Launcher hierarchy: readable labels at the panel's physical size. */
#define LV_FONT_MONTSERRAT_14 1
#define LV_FONT_MONTSERRAT_18 1
#define LV_FONT_MONTSERRAT_20 1
#define LV_FONT_MONTSERRAT_24 1
#define LV_FONT_MONTSERRAT_32 1

#endif
