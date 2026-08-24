#include "vendor_probe.h"

#include <QImage>
#include <dlfcn.h>

#include <atomic>
#include <cerrno>
#include <climits>
#include <cstdio>
#include <cstdlib>
#include <mutex>
#include <vector>

namespace {

using Cleanup = void (*)(void *);
// QImage's external-buffer constructor uses qsizetype for bytesPerLine.
// That is 64-bit on the Paper Pro and 32-bit on the ARMv7 reMarkable 2,
// producing different ELF symbols on the two devices.
using ImageCtor = void (*)(QImage *, unsigned char *, int, int, qsizetype,
                           QImage::Format, Cleanup, void *);

struct Candidate {
    QImage *object;
    unsigned char *pixels;
    int width;
    int height;
    qsizetype stride;
    QImage::Format format;
    Cleanup cleanup;
};

std::atomic<bool> capture{false};
std::mutex candidates_mutex;
std::vector<Candidate> candidates;
std::mutex resolver_mutex;
std::atomic<ImageCtor> real_c1{nullptr};
std::atomic<ImageCtor> real_c2{nullptr};
thread_local bool resolving = false;
void *vendor_instance = nullptr;

ImageCtor resolve_ctor(const char *name, std::atomic<ImageCtor> &slot) {
    ImageCtor cached = slot.load(std::memory_order_acquire);
    if (cached) return cached;
    std::lock_guard<std::mutex> lock(resolver_mutex);
    cached = slot.load(std::memory_order_relaxed);
    if (cached) return cached;
    if (resolving) {
        std::fputs("quill: recursive QImage constructor resolution\n", stderr);
        std::abort();
    }
    resolving = true;
    dlerror();
    cached = reinterpret_cast<ImageCtor>(dlsym(RTLD_NEXT, name));
    const char *error = dlerror();
    resolving = false;
    if (!cached) {
        std::fprintf(stderr, "quill: cannot resolve %s: %s\n", name,
                     error ? error : "unknown error");
        std::abort();
    }
    slot.store(cached, std::memory_order_release);
    return cached;
}

void observe(QImage *image, unsigned char *pixels, Cleanup cleanup) {
    if (!capture.load(std::memory_order_acquire)) return;
    Candidate candidate{image, pixels, image->width(), image->height(),
                        image->bytesPerLine(), image->format(), cleanup};
    std::lock_guard<std::mutex> lock(candidates_mutex);
    candidates.push_back(candidate);
}

void forward_ctor(const char *name, std::atomic<ImageCtor> &slot, QImage *self,
                  unsigned char *pixels, int width, int height, qsizetype stride,
                  QImage::Format format, Cleanup cleanup, void *info) {
    resolve_ctor(name, slot)(self, pixels, width, height, stride, format, cleanup, info);
    observe(self, pixels, cleanup);
}

bool plausible(const Candidate &c) {
    if (!c.object || !c.pixels || c.width <= 0 || c.height <= 0 || c.stride <= 0)
        return false;
    if (c.format != QImage::Format_RGB32) return false;
    const qint64 minimum = static_cast<qint64>(c.width) * 4;
    return c.stride >= minimum && c.stride <= INT_MAX;
}

int requested_index() {
    const char *text = std::getenv("QUILL_AUX_BUFFER_INDEX");
    if (!text || !*text) return -1;
    char *end = nullptr;
    errno = 0;
    long value = std::strtol(text, &end, 10);
    if (errno || *end || value < 0 || value > INT_MAX) return -2;
    return static_cast<int>(value);
}

} // namespace

// Exact Qt constructor entry points. Qt encodes the platform-sized
// bytesPerLine argument as `x` (long long) on AArch64 and `i` (int) on ARMv7.
// Both C1 and C2 are provided because a Qt build may bind either alias.
#if QT_POINTER_SIZE == 4
#define QUILL_QIMAGE_C1 "_ZN6QImageC1EPhiiiNS_6FormatEPFvPvES2_"
#define QUILL_QIMAGE_C2 "_ZN6QImageC2EPhiiiNS_6FormatEPFvPvES2_"
#else
#define QUILL_QIMAGE_C1 "_ZN6QImageC1EPhiixNS_6FormatEPFvPvES2_"
#define QUILL_QIMAGE_C2 "_ZN6QImageC2EPhiixNS_6FormatEPFvPvES2_"
#endif

extern "C" void qimage_external_c1(QImage *self, unsigned char *pixels,
    int width, int height, qsizetype stride, QImage::Format format,
    Cleanup cleanup, void *info) asm(QUILL_QIMAGE_C1);
extern "C" void qimage_external_c1(QImage *self, unsigned char *pixels,
    int width, int height, qsizetype stride, QImage::Format format,
    Cleanup cleanup, void *info) {
    forward_ctor(QUILL_QIMAGE_C1, real_c1, self,
                 pixels, width, height, stride, format, cleanup, info);
}

extern "C" void qimage_external_c2(QImage *self, unsigned char *pixels,
    int width, int height, qsizetype stride, QImage::Format format,
    Cleanup cleanup, void *info) asm(QUILL_QIMAGE_C2);
extern "C" void qimage_external_c2(QImage *self, unsigned char *pixels,
    int width, int height, qsizetype stride, QImage::Format format,
    Cleanup cleanup, void *info) {
    forward_ctor(QUILL_QIMAGE_C2, real_c2, self,
                 pixels, width, height, stride, format, cleanup, info);
}

namespace quill_vendor {

int initialize(FramebufferView *view) {
    if (!view) return 7;
    using Instance = void *(*)();
    dlerror();
    auto instance = reinterpret_cast<Instance>(
        dlsym(RTLD_DEFAULT, "_ZN13EPFramebuffer8instanceEv"));
    if (!instance) {
        const char *error = dlerror();
        std::fprintf(stderr, "quill: EPFramebuffer::instance unavailable: %s\n",
                     error ? error : "unknown error");
        return 3;
    }

    {
        std::lock_guard<std::mutex> lock(candidates_mutex);
        candidates.clear();
    }
    capture.store(true, std::memory_order_release);
    vendor_instance = instance();
    capture.store(false, std::memory_order_release);
    if (!vendor_instance) return 4;

    std::vector<Candidate> valid;
    {
        std::lock_guard<std::mutex> lock(candidates_mutex);
        for (const Candidate &candidate : candidates) {
            if (!plausible(candidate)) continue;
            bool duplicate = false;
            for (const Candidate &existing : valid) {
                if (existing.object == candidate.object && existing.pixels == candidate.pixels) {
                    duplicate = true;
                    break;
                }
            }
            if (!duplicate) valid.push_back(candidate);
        }
    }
    if (valid.empty()) return 5;

    int choice = requested_index();
    if (choice == -2 || (choice >= 0 && choice >= static_cast<int>(valid.size()))) {
        std::fputs("quill: invalid QUILL_AUX_BUFFER_INDEX\n", stderr);
        return 6;
    }
    if (choice < 0 && valid.size() != 1) {
        std::fprintf(stderr, "quill: %zu plausible framebuffer images; selection is ambiguous\n",
                     valid.size());
        for (size_t i = 0; i < valid.size(); ++i)
            std::fprintf(stderr, "quill: candidate %zu: %dx%d stride=%lld format=%d cleanup=%s\n",
                         i, valid[i].width, valid[i].height,
                         static_cast<long long>(valid[i].stride), int(valid[i].format),
                         valid[i].cleanup ? "yes" : "no");
        std::fputs("quill: validate on-device, then set QUILL_AUX_BUFFER_INDEX\n", stderr);
        return 6;
    }
    const Candidate &selected = valid[choice < 0 ? 0 : choice];
    *view = {selected.pixels, selected.width, selected.height,
             selected.stride, selected.format};
    return 0;
}

unsigned long swap(const QRect &rect, int content_type, int mode, bool complete) {
    using Swap = unsigned long (*)(void *, QRect, int, int, int);
    static Swap function = [] {
        auto resolved = reinterpret_cast<Swap>(dlsym(
            RTLD_DEFAULT,
            "_ZN13EPFramebuffer11swapBuffersE5QRect13EPContentType12EPScreenMode6QFlagsINS_10UpdateFlagEE"));
        if (!resolved)
            std::fputs("quill: EPFramebuffer::swapBuffers unavailable\n", stderr);
        return resolved;
    }();
    if (!function || !vendor_instance) return 0;
    return function(vendor_instance, rect, content_type, mode, complete ? 1 : 0);
}

} // namespace quill_vendor
