---
title: The app does not open on Windows
description: What to do when GridCraft flashes in Task Manager and quits, or never shows a window, on Windows 11.
order: 10
verified: 0.5.0
---

If GridCraft 0.5.0 opens a window and closes within a second, start it with the Vulkan graphics backend from PowerShell. On Intel graphics use `gl` instead. If an older version crashes in `igvk64.dll` or `atio6axx.dll`, update to 0.5.0.

## Symptoms

- You start GridCraft and no window appears.
- GridCraft shows up in Task Manager for a moment and then quits.
- The window flashes and GridCraft exits about a tenth of a second later, and `gridcraft.log` ends with `Parent device is lost`.
- Windows Event Viewer logs a crash in `igvk64.dll` (Intel's Vulkan driver) or `atio6axx.dll` (an AMD driver). The Intel crash logs in issues #59 and #120 show exception code `0xc0000005`.

## Fix

If the log ends with `Parent device is lost`:

1. Open PowerShell, go to the folder that contains `gridcraft.exe`, and run:

       $env:WGPU_BACKEND = "vulkan"
       & .\gridcraft.exe
       Remove-Item Env:WGPU_BACKEND

   The last line restores the default for later launches. [Issue #339](https://github.com/storytold/gridcraft/issues/339) confirms that this starts GridCraft 0.5.0.

On Intel graphics use `gl` in place of `vulkan` ([issue #120](https://github.com/storytold/gridcraft/issues/120)). On AMD graphics do not use `gl`: it can bring back the `atio6axx.dll` crash.

### If an older version does not open

1. Download the [latest release](https://github.com/storytold/gridcraft/releases/latest) and [install it](../getting-started/install.md). The reporter of the Intel `igvk64.dll` crash in [issue #59](https://github.com/storytold/gridcraft/issues/59) confirmed it was resolved in 0.5.0.
2. If you must stay on an older version, tell it to use DirectX 12. Open PowerShell, go to the folder that contains `gridcraft.exe`, and run:

       $env:WGPU_BACKEND = "dx12"
       & .\gridcraft.exe
       Remove-Item Env:WGPU_BACKEND

   The last line restores the default for later launches. A maintainer suggested this workaround in issue #59.
3. If 0.5.0 still does not open, report it (see **Still stuck?** below). Do not use `vulkan` on Intel graphics: Vulkan is what crashed there ([issue #59](https://github.com/storytold/gridcraft/issues/59), [issue #120](https://github.com/storytold/gridcraft/issues/120)).

## Why this happens

GridCraft 0.5.0 compiles its DirectX 12 shaders with DXC but does not ship `dxcompiler.dll` and `dxil.dll`, so on some PCs the graphics device is lost before the first frame ([issue #339](https://github.com/storytold/gridcraft/issues/339)). A fix that uses FXC, the compiler built into Windows, was merged after 0.5.0 and is not in a release yet.

On some PCs a graphics driver also crashes while the app is setting up graphics, before the window can open. In issue #59 the crash was in Intel's Vulkan driver, which crashed when asked to list the graphics adapters. For AMD, the README says letting the app also create an OpenGL instance can crash `atio6axx.dll`. That is why 0.5.0 uses DirectX 12 only unless you override it.

## Still stuck?

Follow [Report a problem](get-help.md). GridCraft writes `gridcraft.log` to `%APPDATA%\GridCraft\logs`. Each start moves the previous log to `gridcraft.1.log` and that one to `gridcraft.2.log`, so the log of a run that crashed is still there after the next start. Attach these files when you [open an issue](https://github.com/storytold/gridcraft/issues), or ask in [Discord](https://discord.gg/artcraft). Say which Windows version, graphics card and driver version you have.
