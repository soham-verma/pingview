#!/usr/bin/env bash
# Fetches the built-in offline map: the whole world at zoom 0-7 from a
# pinned Protomaps OpenStreetMap build (ODbL), ~189 MB, into
# basemap-data/world.pmtiles. Too big for git, so builds and CI run this.
#
# Downloads the extract from this repo's `basemap-<BUILD>` release, because
# Protomaps deletes most daily builds within days. If that fails, extracts it
# from Protomaps instead, which needs the `pmtiles` CLI
# (https://github.com/protomaps/go-pmtiles) on PATH, or PMTILES=/path/to/pmtiles.
#
# To move to a newer build: pick a date from
# https://build-metadata.protomaps.dev/builds.json, run this with
# PINGVIEW_BASEMAP_BUILD=<date> SKIP_CHECKSUM=1, upload the result as
# world.pmtiles to a new `basemap-<date>` release, then update BUILD and
# SHA256 below.
set -euo pipefail

PINNED_BUILD=20261013
BUILD="${PINGVIEW_BASEMAP_BUILD:-$PINNED_BUILD}"
MAXZOOM=7
SHA256="3841470e9306787c4d04b1b8e19b2cb7af823e98c5116e7009d09b80c18e1c10"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/basemap-data/world.pmtiles"
PMTILES="${PMTILES:-pmtiles}"
MIRROR="https://github.com/soham-verma/pingview/releases/download/basemap-$BUILD/world.pmtiles"

sha() { shasum -a 256 "$1" 2>/dev/null | awk '{print $1}' || sha256sum "$1" | awk '{print $1}'; }

if [[ -f "$OUT" && "${SKIP_CHECKSUM:-}" != 1 && "$(sha "$OUT")" == "$SHA256" ]]; then
  echo "basemap: $OUT is up to date"
  exit 0
fi

mkdir -p "$(dirname "$OUT")"
if [[ "$BUILD" == "$PINNED_BUILD" ]] && echo "basemap: downloading $MIRROR" \
  && curl -fsSL --retry 3 -o "$OUT.part" "$MIRROR"; then
  :
else
  echo "basemap: extracting world zoom 0-$MAXZOOM from Protomaps build $BUILD"
  "$PMTILES" extract "https://build.protomaps.com/$BUILD.pmtiles" "$OUT.part" --maxzoom="$MAXZOOM"
fi

if [[ "${SKIP_CHECKSUM:-}" != 1 ]]; then
  got="$(sha "$OUT.part")"
  if [[ "$got" != "$SHA256" ]]; then
    rm -f "$OUT.part"
    echo "basemap: checksum mismatch (got $got, want $SHA256)" >&2
    exit 1
  fi
fi
mv "$OUT.part" "$OUT"
echo "basemap: wrote $OUT ($(du -h "$OUT" | cut -f1))"
