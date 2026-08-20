#ifndef LV_CONF_H
#define LV_CONF_H

/* Derived from config/lvgl/lv_conf.h. The differences are the point: they are
 * exactly the parts of that file which were Inkplate-specific rather than
 * product-wide, and finding them is part of what this board is for. */

/* Same as the Inkplate: 8-bit luminance, dithered to 1bpp by the panel driver
 * rather than by LVGL. This is what makes `Panel::blit_l8` a shared seam. */
#define LV_COLOR_DEPTH 8
#define LV_USE_OS LV_OS_NONE
#define LV_DPI_DEF 100

/* An LCD can afford a faster refresh cadence than e-paper's 250ms. */
#define LV_DEF_REFR_PERIOD 30

/* Inkplate routes LVGL's heap into external PSRAM through a custom pool hook.
 * This board has no PSRAM wired up here, so it uses LVGL's builtin allocator
 * over a static arena instead -- smaller, because there is no launcher
 * hierarchy or scene catalogue to hold yet. */
#define LV_MEM_SIZE (48U * 1024U)

/* Retain only the software formats the L8 renderer needs. */
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

#define LV_USE_LOG 0
#define LV_USE_FLOAT 1
#define LV_USE_ANIMIMG 0
#define LV_USE_LOTTIE 0
#define LV_USE_GIF 0
#define LV_THEME_DEFAULT_GROW 0
#define LV_THEME_DEFAULT_TRANSITION_TIME 0

/* No touchscreen on this board, so no gesture recogniser to configure. That is
 * the one part of the Inkplate's UI stack this board cannot exercise. */

#define LV_FONT_MONTSERRAT_14 1
#define LV_FONT_MONTSERRAT_18 1
#define LV_FONT_MONTSERRAT_24 1

#endif
