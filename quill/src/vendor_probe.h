#pragma once

#include <QImage>
#include <QRect>

namespace quill_vendor {

struct FramebufferView {
    unsigned char *pixels = nullptr;
    int width = 0;
    int height = 0;
    qsizetype stride = 0;
    QImage::Format format = QImage::Format_Invalid;
};

// Returns zero on success. The vendor owns the returned storage.
int initialize(FramebufferView *view);
unsigned long swap(const QRect &rect, int content_type, int mode, bool complete);

} // namespace quill_vendor
