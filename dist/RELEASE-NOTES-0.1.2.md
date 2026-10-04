# Dromaius 0.1.2

This maintenance release fixes first-run setup on Intel x64 computers.

## Fix

- **Android emulator download:** Google's package list now identifies Intel
  emulator archives as `x64`. Dromaius now recognises that as equivalent to
  `x86_64`, so a new installation can find and download the official Android
  Emulator on Intel Macs and Windows x64 computers.

## Requirements

- Windows 10 or 11 x64 with Windows Hypervisor Platform, or macOS 12 or later
  on Apple Silicon or Intel.
- 8 GB RAM minimum, 16 GB recommended, and about 12 GB free disk space.
