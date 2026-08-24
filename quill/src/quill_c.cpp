#include "quill.h"
#include "clip_rect.h"
#include "vendor_probe.h"

#include <QCoreApplication>
#include <QEventLoop>
#include <QRect>

#include <cstdio>
#include <limits>
#include <mutex>

namespace {
std::mutex init_mutex;
QCoreApplication *app = nullptr;
quill_vendor::FramebufferView framebuffer;
bool initialized = false;
int last_error = 0;
}

extern "C" {

int quill_init(void) {
    std::lock_guard<std::mutex> lock(init_mutex);
    if (initialized) return 0;
    if (last_error) return last_error;

    app = QCoreApplication::instance();
    if (!app) {
        static int argc = 1;
        static char name[] = "quill";
        static char *argv[] = {name, nullptr};
        // The vendor singleton has no public shutdown operation. Keep the Qt
        // application alive until process teardown rather than destroying it
        // while vendor threads and objects can still reference Qt state.
        app = new QCoreApplication(argc, argv);
    }
    if (!app) return last_error = 1;

    int result = quill_vendor::initialize(&framebuffer);
    if (result) return last_error = result;
    if (!framebuffer.pixels || framebuffer.width <= 0 || framebuffer.height <= 0 ||
        framebuffer.stride <= 0 || framebuffer.stride > std::numeric_limits<int>::max())
        return last_error = 7;

    initialized = true;
    std::fprintf(stderr, "quill: framebuffer %dx%d stride=%lld format=%d\n",
                 framebuffer.width, framebuffer.height,
                 static_cast<long long>(framebuffer.stride), int(framebuffer.format));
    return 0;
}

int quill_width(void) { return initialized ? framebuffer.width : 0; }
int quill_height(void) { return initialized ? framebuffer.height : 0; }
int quill_stride(void) { return initialized ? int(framebuffer.stride) : 0; }
int quill_format(void) { return initialized ? int(framebuffer.format) : -1; }
unsigned char *quill_buffer(void) { return initialized ? framebuffer.pixels : nullptr; }

unsigned long quill_swap_ex(int x, int y, int width, int height, int mode,
                            int full_refresh, int content_type) {
    if (!initialized || width <= 0 || height <= 0) return 0;
    if (content_type != QUILL_CONTENT_MONO && content_type != QUILL_CONTENT_COLOR)
        return 0;

    quill_detail::ClippedRect clipped{};
    if (!quill_detail::clip_rect(x, y, width, height, framebuffer.width,
                                 framebuffer.height, &clipped))
        return 0;

    return quill_vendor::swap(
        QRect(clipped.x, clipped.y, clipped.width, clipped.height),
        content_type, mode, full_refresh != 0);
}

unsigned long quill_swap(int x, int y, int width, int height, int mode,
                         int full_refresh) {
    return quill_swap_ex(x, y, width, height, mode, full_refresh,
                         QUILL_CONTENT_MONO);
}
unsigned long quill_swap_mono_fast(int x, int y, int width, int height) {
    return quill_swap_ex(x, y, width, height, QUILL_MODE_FASTEST, 0,
                         QUILL_CONTENT_MONO);
}
unsigned long quill_swap_mono_quality(int x, int y, int width, int height) {
    return quill_swap_ex(x, y, width, height, QUILL_MODE_COLOR3, 0,
                         QUILL_CONTENT_MONO);
}
unsigned long quill_swap_color(int x, int y, int width, int height) {
    return quill_swap_ex(x, y, width, height, QUILL_MODE_COLOR4, 0,
                         QUILL_CONTENT_COLOR);
}
unsigned long quill_swap_color_full(int x, int y, int width, int height) {
    return quill_swap_ex(x, y, width, height, QUILL_MODE_COLOR4, 1,
                         QUILL_CONTENT_COLOR);
}

void quill_process_events(void) {
    if (initialized && app)
        QCoreApplication::processEvents(QEventLoop::AllEvents, 0);
}

} // extern "C"
