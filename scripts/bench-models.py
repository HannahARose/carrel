#!/usr/bin/env python3
"""Writes the benchmark's invented 3D models (fixtures/bench/items/*.stl) as binary STL.

Each is one closed piece, made by extruding or revolving a profile, so it reads as one part.
Run it from the repository's root after changing a shape; the output is deterministic.
"""

import math
import struct
from pathlib import Path

ITEMS = Path("fixtures/bench/items")


def revolve(profile, segments, turn=0.0):
    """Triangles of a closed (r, z) profile turned about the Z axis."""
    def point(r, z, i):
        a = turn + 2 * math.pi * i / segments
        return (round(r * math.cos(a), 4), round(r * math.sin(a), 4), z)

    triangles = []
    for k in range(len(profile)):
        (r0, z0), (r1, z1) = profile[k], profile[(k + 1) % len(profile)]
        for i in range(segments):
            a, b = point(r0, z0, i), point(r0, z0, i + 1)
            c, d = point(r1, z1, i + 1), point(r1, z1, i)
            triangles += [(a, b, c), (a, c, d)]
    return [t for t in triangles if len(set(t)) == 3]


def extrude(profile, length):
    """Triangles of a closed (y, z) profile, convex from its first corner, along X."""
    triangles = []
    n = len(profile)
    for k in range(n):
        (y0, z0), (y1, z1) = profile[k], profile[(k + 1) % n]
        a, b, c, d = (0, y0, z0), (0, y1, z1), (length, y1, z1), (length, y0, z0)
        triangles += [(a, b, c), (a, c, d)]
    for k in range(1, n - 1):
        (y0, z0), (y1, z1), (y2, z2) = profile[0], profile[k], profile[k + 1]
        triangles.append(((0, y0, z0), (0, y2, z2), (0, y1, z1)))
        triangles.append(((length, y0, z0), (length, y1, z1), (length, y2, z2)))
    return triangles


def write(name, triangles):
    data = bytearray(b"invented benchmark model".ljust(80, b" "))
    data += struct.pack("<I", len(triangles))
    for triangle in triangles:
        data += struct.pack("<3f", 0, 0, 0)
        for corner in triangle:
            data += struct.pack("<3f", *corner)
        data += b"\0\0"
    (ITEMS / name).write_bytes(data)


# An L-shaped bracket, 40 mm long, 20 mm deep and 30 mm high, in 3 mm plate.
write("part_v3_final.stl", extrude([(0, 0), (20, 0), (20, 3), (3, 3), (3, 30), (0, 30)], 40))

# A square tray, 60 mm a side and 20 mm high, with 2 mm walls: four segments turned 45°.
side = 60 / math.sqrt(2)
wall = 2 * math.sqrt(2)
write(
    "print_0412.stl",
    revolve([(0, 0), (side, 0), (side, 20), (side - wall, 20), (side - wall, 2), (0, 2)], 4, math.pi / 4),
)

# A finger ring: 18 mm inside, 2 mm thick, 6 mm wide.
write("ring_v2.stl", revolve([(9, 0), (11, 0), (11, 6), (9, 6)], 96))

# A small figurine: a base, a round body and a head, 44 mm high.
figure = [(0, 0), (12, 0), (12, 3), (9, 4)]
figure += [(10 * math.sin(math.pi * t / 8) + 1, 4 + 20 * t / 8) for t in range(1, 8)]
figure += [(5, 26)]
figure += [(8 * math.sin(math.pi * t / 8), 28 + 16 * (1 - math.cos(math.pi * t / 8)) / 2) for t in range(1, 8)]
figure += [(0, 44)]
write("scan_0007.stl", revolve(figure, 64))
