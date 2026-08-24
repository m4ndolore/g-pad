#pragma once

#ifdef __cplusplus
extern "C" {
#endif

// quill C ABI: direct access to the reMarkable Paper Pro e-paper engine.
// Lifecycle:
//   quill_init();
//   unsigned char *fb = quill_buffer();
//   write RGB/mono pixels into fb;
//   quill_swap*() dirty rectangles to glass.

#define QUILL_CONTENT_MONO  0
#define QUILL_CONTENT_COLOR 1

// Observed useful screen modes on Paper Pro / libqsgepaper ACEP backend.
#define QUILL_MODE_FASTEST 0  // low-latency mono ink
#define QUILL_MODE_FAST    1  // grayscale/mono; not useful for color
#define QUILL_MODE_COLOR3  3  // color-capable
#define QUILL_MODE_COLOR4  4  // color-capable; default color mode
#define QUILL_MODE_COLOR5  5  // color-capable

int quill_init(void);
int quill_width(void);
int quill_height(void);
int quill_stride(void);
int quill_format(void);
unsigned char *quill_buffer(void);

// Raw swap. content_type: QUILL_CONTENT_MONO or QUILL_CONTENT_COLOR.
// Rectangles are clipped to the panel. Beware: full_refresh may be promoted
// to a panel-wide operation by the vendor backend even for a small rectangle;
// use the semantic partial wrappers for interactive/local cleanup work.
unsigned long quill_swap_ex(int x, int y, int w, int h,
                            int mode, int full_refresh, int content_type);

// Compatibility wrapper: mono update, same behavior as the original quill API.
unsigned long quill_swap(int x, int y, int w, int h, int mode, int full_refresh);

// Semantic wrappers learned from on-device probes.
unsigned long quill_swap_mono_fast(int x, int y, int w, int h);
unsigned long quill_swap_mono_quality(int x, int y, int w, int h);
unsigned long quill_swap_color(int x, int y, int w, int h);
unsigned long quill_swap_color_full(int x, int y, int w, int h);

void quill_process_events(void);

#ifdef __cplusplus
}
#endif
