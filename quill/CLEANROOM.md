# Upstream clean-room provenance

The text below is the provenance statement shipped by MaximeRivest/quill
v0.1.0 at commit `39262ee`. This repository vendors those files under MIT and
adds an ARM32 portability extension; it does not claim that the surrounding
Riddle repository has the same fresh-history property.

The framebuffer adapter in `src/vendor_probe.cpp`, `src/vendor_probe.h`,
`src/quill_c.cpp`, and `src/clip_rect.h` was implemented from the behavioral
and ABI specification supplied for this work.

During implementation, the implementer did not inspect the former adapter
source files, upstream epfb-re source, or disassembly/decompilation derived
from epfb-re. The implementation used the specification, existing public
Quill C ABI, Qt headers, normal ELF tooling, and the permitted vendor ABI
symbol names.

This repository was created with fresh history: it contains only the
independently authored files and has never contained the former adapter
sources. The private development repository retains the full working
history as part of the provenance record.

## Permitted references

- The clean-room replacement specification supplied by the project owner
- Qt 6 headers from the matching reMarkable SDK
- ELF metadata and dynamic-linking behavior
- `libqsgepaper.so` obtained by the device owner and not redistributed
- Black-box behavior of the owner's reMarkable Paper Pro

## Verification boundary

Compatibility must be evaluated through the exported C ABI and black-box
acceptance tests. The implementer must not compare this source with the former
implementation. A separate reviewer may perform an independence audit, but
must report only conclusions and externally observable incompatibilities.

This record documents process and authorship; it is not legal advice and does
not determine the terms applicable to the proprietary vendor library.
