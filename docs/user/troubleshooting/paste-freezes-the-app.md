---
title: Clicking Paste freezes the app
description: In GridCraft 0.5.0, clicking the Paste button or its arrow in the ribbon freezes the app. The fix is not in a release yet.
order: 20
verified: 0.5.0
---

In GridCraft 0.5.0, clicking **Paste** or its drop-down arrow on the **Home** tab freezes the app. The cause is fixed in the development version, but not in any release yet, so update to the next release when it comes out.

## Symptoms

- You click **Paste**, or the small arrow next to it, and the window stops responding.
- On Linux, one reporter saw the process end a few seconds later with `SIGABRT`.
- It happens on macOS and Linux. Reports do not say whether it happens on Windows.

## Fix

No release has the fix. Update GridCraft when a release newer than 0.5.0 appears on the [releases page](https://github.com/storytold/gridcraft/releases). Until then, the reports describe the ribbon button and its arrow only, and the issue names no other workaround. Save your work before you click **Paste** (see [Save a workbook or export a copy](../guides/save-and-export.md)).

## Why this happens

A maintainer traced it to the ribbon: when you click **Paste**, the ribbon checks whether you right-clicked while it holds a lock that the app's window library also needs, so the app waits for itself. The clipboard is not involved. The fix was merged after 0.5.0 was released ([issue #273](https://github.com/storytold/gridcraft/issues/273), fix #272).

## Still stuck?

Comment on [issue #273](https://github.com/storytold/gridcraft/issues/273) or ask in [Discord](https://discord.gg/artcraft). Say which system you use.
