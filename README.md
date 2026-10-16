# Concat for Stream Deck+

Stream Deck plugin for the [Concat](https://github.com/jub0t/concat) video editor. Native Rust binary; Apple Silicon, macOS 26+.

## Actions

| Action | Controller | Does |
|--------|-----------|------|
| Command | Key | Sends one editor shortcut (play, split, undo, ripple delete, snap, ...) |
| Jog | Dial | Rotate: step 1 frame (10 while held). Touch: toggle 1/10. Push: configurable (play) |
| Zoom | Dial | Rotate: timeline zoom. Push: configurable (split) |
| Status | Key | API connection state; press brings Concat forward |
| Render | Key or dial | Exports a *closed* project in the background, progress on the key/LCD; press again to cancel |

## How it talks to Concat

- **Editor control** is keystrokes (CoreGraphics `CGEvent`), mirroring the bindings in `concat/ui/app.slint`. Concat is brought forward first. Stream Deck needs Accessibility permission (System Settings > Privacy & Security > Accessibility).
- **Render / Status** use Concat's JSON-RPC API. Enable it in Concat under Settings > Remote and generate a token; the plugin reads address and token from `~/Library/Application Support/app.concat.editor/settings.json`. The API refuses projects open in the window, so Render only works on projects that are not.

## Develop

```sh
mise install
just link     # build, symlink into Stream Deck
just dev      # rebuild + restart plugin
just logs     # tail dev.misfit.concat.sdPlugin/logs/plugin.log
just check    # fmt, clippy, manifest validation
just pack     # dist/dev.misfit.concat.streamDeckPlugin
```

## Release

Push a `vX.Y.Z` tag matching `Cargo.toml` (and `X.Y.Z.0` in the manifest), or `vX.Y.Z-rc.N` for a pre-release. The release workflow signs the binary with Developer ID, notarizes it, packs the plugin, and publishes it with a GitHub artifact attestation and SLSA Build Level 3 provenance.

Verify a download:

```sh
gh attestation verify dev.misfit.concat-X.Y.Z.streamDeckPlugin --repo misfitdev/streamdeck-concat
slsa-verifier verify-artifact dev.misfit.concat-X.Y.Z.streamDeckPlugin \
  --provenance-path dev.misfit.concat-X.Y.Z.streamDeckPlugin.intoto.jsonl \
  --source-uri github.com/misfitdev/streamdeck-concat --source-tag vX.Y.Z
```

Repository secrets: `DEVELOPER_ID_P12`, `DEVELOPER_ID_P12_PASSWORD`, `NOTARY_KEY_ID`, `NOTARY_ISSUER_ID`, `NOTARY_KEY`.

## License

MIT OR Apache-2.0
