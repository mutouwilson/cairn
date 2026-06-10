# `share-extension/` — macOS Share Sheet target (scaffold)

**Status:** scaffold only. Producing a shipping Share Extension is a chunk of
Xcode work that doesn't fit cleanly inside the Tauri toolchain. This file
documents the pieces needed; an actual `.appex` bundle is filed as Phase 6.

## What ships when this is done

A user, anywhere on macOS, can:

1. Highlight text / select an image / be on a web page in Safari.
2. Click the Share button.
3. Pick **Cairn** from the share targets.
4. The selected content lands in their Cairn memory as a new note with
   `source = "share-sheet"`, attribution preserved (source app bundle id +
   URL if available).

## Why it's not in the main Tauri build

Tauri 2 packages a single `.app` bundle. macOS share extensions require:

- A separate `NSExtension` target (`.appex`) shipped *inside* the `.app/Contents/PlugIns/`.
- An **App Group** identifier shared with the main app.
- Code signing with the developer team that owns the main app.
- A small Swift entry point implementing `NSExtensionRequestHandling`.

Tauri's `tauri-build` doesn't produce extension targets. We have two options:

### Option A — Xcode post-build step

After `tauri build`, run `xcodebuild` with the extension project, then `cp`
the resulting `.appex` into `target/release/bundle/macos/Cairn.app/Contents/PlugIns/`.
Signed and notarised with the main app. This is the path most other
non-Apple-first apps take (1Password, Anki, etc.).

### Option B — Swift Package + cargo-bundle hook

Use the Swift Package Manager to build the extension as a standalone target,
hook it into `cargo bundle` via a custom build script. More fragile, more
custom.

We'll likely take Option A in Phase 6 because it's the well-trodden path.

## The Swift extension stub

```swift
import UIKit
import Social
import MobileCoreServices

final class ShareViewController: SLComposeServiceViewController {
    override func didSelectPost() {
        guard let item = extensionContext?.inputItems.first as? NSExtensionItem,
              let attachments = item.attachments else {
            return
        }
        for attachment in attachments {
            if attachment.hasItemConformingToTypeIdentifier(kUTTypePlainText as String) {
                attachment.loadItem(forTypeIdentifier: kUTTypePlainText as String,
                                    options: nil) { [weak self] data, _ in
                    guard let text = data as? String else { return }
                    self?.handOff(text: text)
                }
            }
        }
    }

    private func handOff(text: String) {
        let groupId = "group.so.cairn.app"
        let defaults = UserDefaults(suiteName: groupId)
        var pending = defaults?.array(forKey: "pendingShares") as? [[String: Any]] ?? []
        pending.append([
            "text": text,
            "source": "share-sheet",
            "ts": Date().timeIntervalSince1970,
        ])
        defaults?.set(pending, forKey: "pendingShares")
        extensionContext?.completeRequest(returningItems: nil, completionHandler: nil)
    }
}
```

## The Cairn-side handler

Cairn's main process will, on launch and on focus, drain the App Group's
`pendingShares` array:

```rust
// src/share_sheet.rs (Phase 6)
use serde::Deserialize;

#[derive(Deserialize)]
struct PendingShare {
    text: String,
    source: String,
    ts: f64,
}

pub fn drain_pending_shares(/* ... */) {
    // Read UserDefaults("group.so.cairn.app").pendingShares via
    // `objc2-foundation` or a Swift bridge. For each, call the same
    // `capture::processor::process_note` we use everywhere else.
}
```

## Open questions / risks

- App Group identifiers require a developer team prefix; we'll need to commit
  to one for distribution. For now, dev builds can use `group.so.cairn.dev`.
- Sandbox: share extensions run in a tighter sandbox than the main app; the
  hand-off via UserDefaults App Group is the standard escape valve.
- iOS: the same `.appex` model applies; if/when we ship a Tauri iOS variant,
  this codebase ports cleanly.

## What to read

- Apple — [Creating an action extension](https://developer.apple.com/library/archive/documentation/General/Conceptual/ExtensibilityPG/Share.html)
- Tauri 2 macOS bundle internals — `src-tauri/tauri.conf.json#bundle`
