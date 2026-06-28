# Barony SDK Notes

Date: 2026-06-28

Primary design spec:

- `docs/superpowers/specs/2026-06-28-barony-runtime-sdk-design.md`

## Goal

Build a Rust-based SDK layer for Barony mods that lets mod authors write
TypeScript scripts through `ts_embed_vm`, without relying on executable
replacement as the normal modding workflow.

## Current understanding

- Barony's existing mod system is mostly data and asset replacement through
  mounted mod folders.
- Deep gameplay behavior is still implemented in C++.
- Existing mods do not appear to be native C++ plugins loaded by the game.
- `ts_embed_vm` can provide a Rust-owned TypeScript runtime, generated SDK
  declarations, host functions, callbacks, script loading, and a contract
  registry.
- The SDK should expose data registries first, then behavior hooks where the
  engine has safe extension points.

## Important Barony mod loading points

- `src/init.cpp` mounts the base data folders and output mod directory through
  PhysFS.
- `src/mod_tools.hpp` declares the `Mods` state used to track mounted paths,
  local mods, workshop mods, reload flags, and mod load/unload.
- `src/mod_tools.cpp` mounts and unmounts mod paths, reloads data, assets,
  language files, models, sounds, sprites, music, lights, and game data files.
- `src/ui/MainMenu.cpp` contains local/workshop mod UI loading and calls into
  `Mods::loadMods()`.
- `src/init_game.cpp` reloads many data-driven systems during mod reload.

## Current class extension findings

- `NUMCLASSES` is fixed to 26 in `src/game.hpp`.
- Class IDs are hardcoded as C++ enum constants.
- `initClassStats()` in `src/charclass.cpp` hardcodes stats and proficiencies.
- `initClass()` in `src/charclass.cpp` hardcodes starting equipment, spells,
  hotbars, and class-specific setup.
- `classStatGrowth` in `src/monster.hpp` is a fixed vector list matching native
  class order.
- Main menu class selection uses a fixed class ordering in `src/ui/MainMenu.cpp`.
- Some network paths already treat class IDs as 32-bit values, but packet paths
  still need an audit before dynamic class IDs are considered multiplayer-safe.

## Preferred direction

Introduce a runtime class registry instead of extending the enum directly.

- Keep native Barony class IDs stable.
- Register the 26 vanilla classes as native registry entries.
- Append modded classes from SDK data.
- Use stable string IDs such as `example_mod:super_pirates` for persistence.
- Map stable IDs to runtime numeric IDs only after all active mods are loaded.
- Drive class UI, class stats, start loadouts, descriptions, and growth from the
  registry.
- Keep C++ fallback hooks for vanilla classes that still have special behavior.

## Loader direction

Preferred route as of 2026-06-28:

- Target an external runtime SDK, not a normal source-patch-only fork.
- Use the Barony mod acceptance/load path as the trigger point when possible.
- Once the SDK runtime is loaded, it should be able to own higher-level
  registries and script hooks.
- If the normal mod path cannot load native code, use a loader fallback such as
  `dinput8.dll` injection.
- Treat the Barony C++ source as a map of engine behavior and data structures,
  not as the final distribution mechanism.
- Treat Ghidra comparisons against the installed game as required validation
  for binary/runtime hooks.

## Test mod idea

Create a minimal test mod that copies the behavior shape of an existing class,
probably Jester, but registers it as a new class:

- Stable ID: `barony_sdk_test:super_pirates`
- Display name: `Super Pirates`
- Source template: vanilla Jester
- Stats: intentionally exaggerated
- Start loadout: one silly or overpowered weapon
- Purpose: prove that a new class can appear in the menu, start a run, receive
  custom stats/items, and survive a basic reload path.

## Open questions

- Which exact retail Barony executable version is installed?
- Which function signatures are stable enough for the first menu and class-init
  hooks?
- Which Jester loadout and item constants should `Super Pirates` copy before
  applying its exaggerated overrides?
- Where should SDK mods live for the first prototype: inside Barony's normal
  `mods` folder, or in a dedicated `barony-sdk` folder next to the loader?
