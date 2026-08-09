#!/usr/bin/env python3

"""Inject STARTUP.NSH into the root of Microsoft's FAT12 EFI boot image."""

from __future__ import annotations

import struct
import sys
from pathlib import Path


def u16(data: bytearray, offset: int) -> int:
    return struct.unpack_from("<H", data, offset)[0]


def fat12_get(fat: bytearray, cluster: int) -> int:
    offset = cluster + cluster // 2
    if cluster & 1:
        return ((fat[offset] >> 4) | (fat[offset + 1] << 4)) & 0xFFF
    return (fat[offset] | ((fat[offset + 1] & 0x0F) << 8)) & 0xFFF


def fat12_set(fat: bytearray, cluster: int, value: int) -> None:
    offset = cluster + cluster // 2
    value &= 0xFFF
    if cluster & 1:
        fat[offset] = (fat[offset] & 0x0F) | ((value << 4) & 0xF0)
        fat[offset + 1] = (value >> 4) & 0xFF
    else:
        fat[offset] = value & 0xFF
        fat[offset + 1] = (fat[offset + 1] & 0xF0) | ((value >> 8) & 0x0F)


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit("usage: inject-startup.py EFI_IMAGE STARTUP_NSH")

    image_path = Path(sys.argv[1])
    script = Path(sys.argv[2]).read_bytes()
    data = bytearray(image_path.read_bytes())

    bytes_per_sector = u16(data, 11)
    sectors_per_cluster = data[13]
    reserved_sectors = u16(data, 14)
    fat_count = data[16]
    root_entries = u16(data, 17)
    total_sectors = u16(data, 19) or struct.unpack_from("<I", data, 32)[0]
    sectors_per_fat = u16(data, 22)
    if bytes_per_sector != 512 or sectors_per_cluster == 0 or fat_count != 2:
        raise SystemExit("unexpected EFI FAT layout")

    root_sectors = (root_entries * 32 + bytes_per_sector - 1) // bytes_per_sector
    fat_offset = reserved_sectors * bytes_per_sector
    fat_size = sectors_per_fat * bytes_per_sector
    root_offset = (reserved_sectors + fat_count * sectors_per_fat) * bytes_per_sector
    data_sector = reserved_sectors + fat_count * sectors_per_fat + root_sectors
    cluster_size = sectors_per_cluster * bytes_per_sector
    cluster_count = (total_sectors - data_sector) // sectors_per_cluster

    fat = bytearray(data[fat_offset : fat_offset + fat_size])
    needed = max(1, (len(script) + cluster_size - 1) // cluster_size)
    free_clusters = [
        cluster for cluster in range(2, cluster_count + 2) if fat12_get(fat, cluster) == 0
    ][:needed]
    if len(free_clusters) != needed:
        raise SystemExit("not enough free clusters in EFI image")

    name = b"STARTUP NSH"
    entry_offset = None
    for index in range(root_entries):
        offset = root_offset + index * 32
        first = data[offset]
        if data[offset : offset + 11] == name:
            entry_offset = offset
            break
        if entry_offset is None and first in (0x00, 0xE5):
            entry_offset = offset
    if entry_offset is None:
        raise SystemExit("no free FAT root directory entry")

    for index, cluster in enumerate(free_clusters):
        next_cluster = free_clusters[index + 1] if index + 1 < len(free_clusters) else 0xFFF
        fat12_set(fat, cluster, next_cluster)
        chunk = script[index * cluster_size : (index + 1) * cluster_size]
        offset = (data_sector + (cluster - 2) * sectors_per_cluster) * bytes_per_sector
        data[offset : offset + cluster_size] = chunk.ljust(cluster_size, b"\0")

    for copy in range(fat_count):
        offset = fat_offset + copy * fat_size
        data[offset : offset + fat_size] = fat

    entry = bytearray(32)
    entry[:11] = name
    entry[11] = 0x20
    struct.pack_into("<H", entry, 26, free_clusters[0])
    struct.pack_into("<I", entry, 28, len(script))
    data[entry_offset : entry_offset + 32] = entry
    image_path.write_bytes(data)


if __name__ == "__main__":
    main()
