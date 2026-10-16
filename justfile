plugin := "dev.misfit.concat"
sdplugin := plugin + ".sdPlugin"
target := "aarch64-apple-darwin"
dist := "target/dist"
# "-" signs ad hoc; releases set a Developer ID identity
sign_identity := env("CONCAT_SIGN_IDENTITY", "-")

default:
    @just --list

# Build the plugin binary into the .sdPlugin folder
build:
    cargo build --release --target {{target}}
    mkdir -p {{sdplugin}}/bin
    cp target/{{target}}/release/concat-plugin {{sdplugin}}/bin/concat-plugin

# Lint and check formatting
lint:
    cargo clippy --target-dir target/clippy --all-targets -- -D warnings
    cargo fmt --check

# Format the code
fmt:
    cargo fmt

# Run the test suite
test:
    cargo test

# Validate the plugin manifest against Elgato's schema
validate: build
    streamdeck validate {{sdplugin}}

# Everything CI would check
check: lint test validate

# Dependency licenses and advisories, and workflow security
audit:
    cargo deny check
    zizmor .github/workflows
    actionlint

# Symlink the plugin into Stream Deck (once)
link: build
    streamdeck link {{sdplugin}}

# Rebuild and restart the plugin in Stream Deck
dev: build
    streamdeck restart {{plugin}}

watch:
    watchexec -w src -w Cargo.toml -- just dev

logs:
    tail -f {{sdplugin}}/logs/plugin.log

# Sign, notarize (when NOTARY_KEY_ID is set) and pack the plugin into target/dist
package: build
    #!/usr/bin/env bash
    set -euo pipefail
    version="$(awk -F'"' '/^version = / {print $2; exit}' Cargo.toml)"
    manifest="$(jq -r .Version {{sdplugin}}/manifest.json)"
    if [ "$manifest" != "$version.0" ]; then
        echo "manifest Version $manifest does not match Cargo.toml $version" >&2
        exit 1
    fi
    # release tags may carry -rc.N; the manifest only takes X.Y.Z.W
    label="${RELEASE_TAG:-v$version}"
    label="${label#v}"
    binary="{{sdplugin}}/bin/concat-plugin"
    identity="{{sign_identity}}"
    mkdir -p "{{dist}}"
    rm -f "{{dist}}"/{{plugin}}*
    [ "$identity" = "-" ] || timestamp="--timestamp"
    codesign --force --sign "$identity" --identifier {{plugin}}.plugin \
        --options runtime ${timestamp:+"$timestamp"} "$binary"
    codesign --verify --strict --verbose=2 "$binary"
    if [ -n "${NOTARY_KEY_ID:-}" ]; then
        # A bare binary cannot be stapled; Gatekeeper fetches the ticket online.
        stage="$(mktemp -d)"
        trap 'rm -rf "$stage"' EXIT
        ditto -c -k "$binary" "$stage/concat-plugin.zip"
        result="$(xcrun notarytool submit "$stage/concat-plugin.zip" --key "$NOTARY_KEY_PATH" \
            --key-id "$NOTARY_KEY_ID" --issuer "$NOTARY_ISSUER_ID" --wait --output-format json)"
        echo "$result"
        if [ "$(jq -r .status <<<"$result")" != "Accepted" ]; then
            xcrun notarytool log "$(jq -r .id <<<"$result")" --key "$NOTARY_KEY_PATH" \
                --key-id "$NOTARY_KEY_ID" --issuer "$NOTARY_ISSUER_ID" >&2
            exit 1
        fi
    fi
    streamdeck pack {{sdplugin}} --output "{{dist}}" --force
    out="{{dist}}/{{plugin}}-$label.streamDeckPlugin"
    mv "{{dist}}/{{plugin}}.streamDeckPlugin" "$out"
    (cd "{{dist}}" && shasum -a 256 "$(basename "$out")" > "$(basename "$out").sha256")
    ls -l "{{dist}}"

# Release notes for the tagged commit, or for unreleased commits, from conventional commits
notes:
    @git cliff --current --strip header 2>/dev/null || git cliff --unreleased --strip header
