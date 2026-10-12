---
title: The app does not open on Windows
description: What to do when GridCraft flashes in Task Manager and quits, or never shows a window, on Windows 11.
order: 10
verified: 0.5.0
---

Install GridCraft 0.5.0 or newer. From 0.5.0 on, the Windows app starts with DirectX 12 only, which avoids the graphics-driver crashes that kept earlier versions from opening.

## Symptoms

- You start GridCraft and no window appears.
- GridCraft shows up in Task Manager for a moment and then quits.
- Windows Event Viewer logs a crash in `igvk64.dll` (Intel's Vulkan driver) or `atio6axx.dll` (an AMD driver), with exception code `0xc0000005`.

## Fix

1. Download the [latest release](https://github.com/storytold/gridcraft/releases/latest) and [install it](../getting-started/install.md). The reporter of the Intel `igvk64.dll` crash in [issue #59](https://github.com/storytold/gridcraft/issues/59) confirmed it was resolved in 0.5.0.
2. If you must stay on an older version, tell it to use DirectX 12. Open PowerShell, go to the folder that contains `gridcraft.exe`, and run:

       $env:WGPU_BACKEND = "dx12"
       & .\gridcraft.exe
       Remove-Item Env:WGPU_BACKEND

   The last line restores the default for later launches. A maintainer suggested this workaround in issue #59.
3. If 0.5.0 still does not open, try another graphics backend the same way, with `vulkan` in place of `dx12`. Try one at a time.

Be careful with the override. Setting `WGPU_BACKEND` to `gl` can bring back the AMD driver crash, and setting it to `vulkan` can bring back the Intel one.

## Why this happens

On some PCs a graphics driver crashes while the app is setting up graphics, before the window can open. In issue #59 the crash was in Intel's Vulkan driver, which crashed when asked to list the graphics adapters. For AMD, the README says letting the app also create an OpenGL instance can crash `atio6axx.dll`. That is why 0.5.0 uses DirectX 12 only unless you override it.

## Still stuck?

GridCraft writes a log file called `gridcraft.log` to `%APPDATA%\GridCraft\logs`. Each start moves the previous log to `gridcraft.1.log` and that one to `gridcraft.2.log`, so the log of a run that crashed is still there after the next start. Attach these files when you [open an issue](https://github.com/storytold/gridcraft/issues), or ask in [Discord](https://discord.gg/artcraft). Say which Windows version, graphics card and driver version you have.
