# Hardware parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-14 · **Change:** minor (Force Touch pressure in Draw with Trackpad; no UI stall when the mode starts) · **Target:** Microsoft Excel (Microsoft 365), Excel for Mac 16.113.4

The hardware Excel uses, per platform, against GridCraft. Part of
[target-app-parity.md](target-app-parity.md). **Hardware: ~40% ready, 25–40 h** (estimated).

| Hardware feature | Excel | GridCraft (macOS · Windows · Linux · BSD · web) | Gap and estimate |
|---|---|---|---|
| Multi-core recalculation | Multi-threaded recalculation on all cores (Windows and Mac), configurable thread count | Single-threaded everywhere (no threading in `crates/calc`) | Parallel recalc of independent dependency chains on native builds; WASM stays single-threaded or uses workers later: 12–18 h |
| GPU rendering of the grid and charts | DirectX / Metal accelerated drawing | wgpu: Metal · DirectX 12 (default since 10-08; OpenGL when DirectX 12 has no GPU driver or fails to start; Vulkan only on request, so the Intel UHD `igvk64.dll` crash (#120) can't happen; shaders compiled with FXC, so no DXC runtime is needed, #339) · Vulkan/GL · Vulkan/GL · WebGPU or WebGL2 | At parity in kind. Driver problems: screen flashing on Windows 11 (#76): 2–4 h |
| Pen and touch (Draw tab) | Pen pressure and tilt, eraser end, touch mode, Ink to Shape, ink replay | Pen, highlighter, eraser, lasso, Ink to Shape with mouse/pen input; imported InkML pressure and inverse-speed width supported; **no live stylus pressure or tilt**, no touch mode | Pressure-sensitive strokes where winit reports force; touch-friendly ribbon spacing: 4–6 h |
| Trackpad and mouse | Smooth scrolling, pinch zoom, horizontal scroll, Magic Mouse, Draw with Trackpad | Smooth scrolling/zoom via egui; Draw contact capture via public AppKit touches on macOS (Force Touch: stage-1 pressure while the pad is clicked down thickens the line, `NSPressureBehavior.primaryGeneric` so there is no force click), evdev on Linux, readable precision-touchpad HID on Windows, probed when the mode is turned on (not at launch) and released when it is turned off. Floating canvas, one-finger draw, two-finger pan, any-key exit. Checks device/access; no cursor fallback | Linux requires input-device access; Windows HID depends on driver access. macOS/Windows/Linux compilation and synthetic-contact tests pass; physical-device cross-platform testing is pending. Pinch-zoom verification remains: 1–2 h |
| Multiple windows and monitors | One window per workbook, New Window for the same workbook, per-monitor DPI | One window; multiple workbooks switch inside it (#162; PR #171) | Native window per workbook, New Window, Arrange All across monitors: 6–8 h |
| High-DPI and display scaling | yes | yes (display scale matched to desktop density; View › Interface size 75–200 % on top, #230) | — |
| Accessibility hardware (screen readers, braille displays, switch control) | Narrator, JAWS, NVDA, VoiceOver | AccessKit enabled in eframe; grid cells and ribbon not yet exposed as an accessible tree | Counted under UI ([ui-parity.md](ui-parity.md)) |
| Printers | System print with driver options | Desktop submits through craft-print with printer choice, copies, duplex and grayscale; web browser printing | Counted under print |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-14 | minor | Draw with Trackpad on macOS: Force Touch pressure sets the line width (touch = lightest, clicking down and pressing harder thickens); touch events are drained in a private run-loop mode, which removes the 1–2 s UI stall when the mode starts |
| 2026-10-12 | patch | Draw with Trackpad probes its device on first use instead of at launch, closes it when turned off, explains missing Linux input permission; trackpad crates pinned exactly |
| 2026-10-12 | minor | Desktop File > Print now discovers printers and submits PDF jobs through pinned craft-print with printer choice, copies, duplex and grayscale; browser printing and PDF export remain available. Remaining print parity includes page ranges and driver-specific tray/options UI; physical printer fidelity needs hardware validation. |
| 2026-10-11 | trivial | Windows: DirectX 12 shaders compiled with FXC, no DXC runtime needed (#339, #354) |
| 2026-10-11 | patch | Fixed macOS trackpad abort: reject native mouse/key events before querying AppKit touches; native mouse-event regression test |
| 2026-10-11 | minor | Native Draw contact backends and capability/access checks; physical Windows/Linux validation pending |
| 2026-10-11 | trivial | Windows: OpenGL fallback when DirectX 12 has no GPU or fails to start; Vulkan never by default (#120) |
| 2026-10-10 | major | Created: single-threaded calc, wgpu rendering, no pen pressure, single window |
