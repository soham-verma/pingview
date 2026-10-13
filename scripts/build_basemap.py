#!/usr/bin/env python3
"""Builds the offline basemap (src/basemap/basemap.json) from Natural Earth.

Natural Earth is public domain (https://www.naturalearthdata.com). Download
the GeoJSON layers below from
https://github.com/nvkelso/natural-earth-vector/tree/master/geojson into one
folder, then run:

    pip install shapely
    python3 scripts/build_basemap.py <natural-earth-folder>

World layers use the 1:50m data; Australia gets the more detailed 1:10m
coastline, state borders, lakes, major roads and towns, clipped to its
bounding box. Coordinates are rounded to keep the file small.
"""

import json
import sys
from pathlib import Path

from shapely.geometry import box, mapping, shape

AU_BOX = box(112.0, -44.5, 154.5, -9.0)
OUT = Path(__file__).resolve().parents[1] / "src" / "basemap" / "basemap.json"


def load(src: Path, name: str):
    return json.loads((src / f"{name}.geojson").read_text())["features"]


def rounded(coords, digits):
    if isinstance(coords[0], (int, float)):
        return [round(coords[0], digits), round(coords[1], digits)]
    return [rounded(c, digits) for c in coords]


def feature(geom, digits, props=None):
    g = mapping(geom)
    return {
        "type": "Feature",
        "properties": props or {},
        "geometry": {"type": g["type"], "coordinates": rounded(g["coordinates"], digits)},
    }


def layer(features, digits, clip=None, keep=lambda f: True, props=lambda f: None, simplify=0.0):
    out = []
    for f in features:
        if not f.get("geometry") or not keep(f):
            continue
        geom = shape(f["geometry"])
        if clip is not None:
            if not geom.intersects(clip):
                continue
            geom = geom.intersection(clip)
        if simplify:
            geom = geom.simplify(simplify, preserve_topology=True)
        if geom.is_empty:
            continue
        out.append(feature(geom, digits, props(f)))
    return {"type": "FeatureCollection", "features": out}


def main(src: Path):
    p = lambda f: f["properties"]  # noqa: E731
    data = {
        "world_land": layer(load(src, "ne_50m_land"), 2, simplify=0.02),
        "world_lakes": layer(load(src, "ne_50m_lakes"), 2, simplify=0.02),
        "world_borders": layer(load(src, "ne_50m_admin_0_boundary_lines_land"), 2, simplify=0.02),
        "au_land": layer(load(src, "ne_10m_land"), 3, clip=AU_BOX, simplify=0.002),
        "au_states": layer(
            load(src, "ne_10m_admin_1_states_provinces_lines"),
            3,
            clip=AU_BOX,
            keep=lambda f: p(f).get("ADM0_A3") == "AUS",
            simplify=0.002,
        ),
        "au_lakes": layer(load(src, "ne_10m_lakes"), 3, clip=AU_BOX, simplify=0.002),
        "au_roads": layer(
            load(src, "ne_10m_roads"),
            3,
            clip=AU_BOX,
            # Most roads have no country code; the clip box does the rest.
            keep=lambda f: p(f).get("continent") == "Oceania"
            and p(f).get("type") in ("Major Highway", "Secondary Highway"),
            props=lambda f: {"major": p(f).get("type") == "Major Highway"},
            simplify=0.002,
        ),
        "au_places": layer(
            load(src, "ne_10m_populated_places_simple"),
            3,
            keep=lambda f: p(f).get("adm0name") == "Australia",
            props=lambda f: {"name": p(f)["name"], "rank": p(f).get("scalerank", 10)},
        ),
    }
    data["_source"] = "Natural Earth (public domain), naturalearthdata.com"
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(data, separators=(",", ":")))
    sizes = {k: len(v["features"]) for k, v in data.items() if isinstance(v, dict)}
    print(f"wrote {OUT} ({OUT.stat().st_size / 1e6:.2f} MB): {sizes}")


if __name__ == "__main__":
    main(Path(sys.argv[1]))
