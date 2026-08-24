#pragma once

#include <algorithm>
#include <cstdint>

namespace quill_detail {

struct ClippedRect {
    int x;
    int y;
    int width;
    int height;
};

inline bool clip_rect(int x, int y, int width, int height,
                      int framebuffer_width, int framebuffer_height,
                      ClippedRect *result) {
    if (!result || width <= 0 || height <= 0 ||
        framebuffer_width <= 0 || framebuffer_height <= 0)
        return false;

    const int64_t right = int64_t(x) + int64_t(width);
    const int64_t bottom = int64_t(y) + int64_t(height);
    const int64_t x0 = std::max<int64_t>(0, x);
    const int64_t y0 = std::max<int64_t>(0, y);
    const int64_t x1 = std::min<int64_t>(framebuffer_width, right);
    const int64_t y1 = std::min<int64_t>(framebuffer_height, bottom);
    if (x1 <= x0 || y1 <= y0) return false;

    *result = {int(x0), int(y0), int(x1 - x0), int(y1 - y0)};
    return true;
}

} // namespace quill_detail
