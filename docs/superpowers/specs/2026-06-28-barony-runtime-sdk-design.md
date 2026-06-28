# Barony Runtime SDK Design

Date: 2026-06-28

## Objective

Build the first proof-of-control for a Barony SDK that is written in Rust,
embeds `ts_embed_vm`, and lets a TypeScript mod register a new playable class
without replacing an existing vanilla class.

The test mod is `Super Pirates`: a new class based on Jester, renamed, with
exaggerated stats and an intentionally overpowered starting weapon.

## Non-goals for the first MVP

- No multiplayer support guarantee.
- No Workshop packaging guarantee.
- No general item, monster, spell, or map registry yet.
- No Barony source fork as the final mod distribution path.
- No broad binary patching beyond the minimum hooks needed for the class proof.

## Distribution Strategy

The SDK should be a runtime layer loaded into the retail Barony process.

Preferred bootstrap:

1. Try to use Barony's normal mod loading path as a trigger if a native entry
   point exists.
2. If Barony only accepts data/assets, ship a `dinput8.dll` proxy loader in the
   Barony install directory.
3. The proxy forwards calls to the real system `dinput8.dll`, then initializes
   the Rust SDK runtime.

The Barony C++ source is used as a map of engine behavior. Ghidra comparison
against the installed executable is used to identify and validate function
addresses, call sites, structs, and version-sensitive hook points.

## Architecture

### Rust SDK Loader

The loader is a Rust `cdylib` built as `dinput8.dll`.

Responsibilities:

- Load and forward the real Windows `dinput8.dll` exports.
- Initialize logging early.
- Locate the Barony process base module and game directory.
- Initialize the TypeScript VM through `ts_embed_vm`.
- Discover SDK-enabled mods from the Barony `mods` folder or a dedicated
  `barony-sdk` folder.
- Load each mod's TypeScript entrypoint.
- Own the SDK registries used by hooks.

### TypeScript VM

`ts_embed_vm` is the embedded runtime.

Responsibilities:

- Generate the SDK declaration files for mod authors.
- Load TypeScript mod entrypoints.
- Expose host APIs such as `classes.register(...)`.
- Validate mod declarations before they reach native hooks.
- Keep script state alive while the game is running.

The MVP can use synchronous host contracts only. Async hooks can be deferred
until the engine integration points need them.

### Class Registry

The class registry is the first real SDK registry.

It stores:

- Stable string ID, such as `barony_sdk_test:super_pirates`.
- Runtime numeric ID.
- Display name.
- Base class template, such as Jester.
- Stats and proficiencies.
- Starting items.
- Starting spells.
- Hotbar layout.
- Description text.
- Optional tags and future behavior hook names.

Native classes keep their original IDs. Modded classes are appended at runtime.
The registry maps stable string IDs to runtime IDs after active mods are loaded.

### Binary Hook Layer

The hook layer connects Barony C++ behavior to the Rust registry.

MVP hooks:

- Class list/count hook for the main menu.
- Class display/description hook for the selected class.
- Class initialization hook to apply stats, proficiencies, items, spells, and
  hotbar when `Super Pirates` starts a run.

The hook layer should be thin. It should query the Rust registry and avoid
duplicating class data in patch code.

## Super Pirates Test Mod

Example TypeScript shape:

```ts
classes.register({
  id: "barony_sdk_test:super_pirates",
  name: "Super Pirates",
  baseClass: "jester",
  stats: {
    str: 99,
    dex: 99,
    con: 99,
    int: 20,
    per: 20,
    chr: 99
  },
  hp: 300,
  mp: 150,
  proficiencies: {
    sword: 100,
    axe: 100,
    swimming: 100
  },
  startItems: [
    { item: "ARTIFACT_SWORD", count: 1 },
    { item: "FOOD_CREAMPIE", count: 20 }
  ],
  description: "A deeply unserious menace with royal-level pirate energy."
});
```

The exact item constants must be verified against Barony's item enum and item
tables before implementation.

## Data Flow

1. Barony starts.
2. Windows loads the SDK `dinput8.dll` proxy.
3. The proxy loads the real `dinput8.dll` and initializes Rust state.
4. Rust initializes `ts_embed_vm`.
5. Rust discovers SDK test mod files.
6. TypeScript registers `Super Pirates`.
7. Rust validates and stores the class definition.
8. Hooks make the class visible in the main menu.
9. When the player starts a run with `Super Pirates`, hooks apply the modded
   class definition.

## Error Handling

- If the real `dinput8.dll` cannot be loaded, log and fail early.
- If TypeScript compilation fails, disable that SDK mod and continue boot if
  possible.
- If a class registration is invalid, reject that class and report the reason.
- If hooks cannot resolve expected Barony symbols or signatures, disable the SDK
  runtime for that game version.
- If a save references a missing modded class, show a clear missing-mod error
  once save integration exists.

## Version Safety

The runtime must assume the retail executable can change.

Required safety checks:

- Barony executable hash or build identifier.
- Signature match before installing each hook.
- Hook install success/failure logging.
- A compatibility table for known executable versions.
- A no-hook fallback if the installed version is unknown.

## Testing Strategy

Initial tests:

- Rust unit tests for class definition validation.
- Rust tests for TS host contract registration shape.
- SDK generation test that emits TypeScript declarations for `classes`.
- Manual smoke test: Barony boots with `dinput8.dll` present.
- Manual smoke test: SDK logs show `Super Pirates` registered.
- Manual smoke test: class appears in the class selection menu.
- Manual smoke test: starting a run applies custom stats and item loadout.

Later tests:

- Save/load round trip.
- Missing mod recovery.
- Multiple SDK mods registering classes.
- Version mismatch handling.
- Multiplayer host/client mod registry checks.

## Implementation Phases

1. Document and validate hook targets using source and Ghidra.
2. Scaffold Rust `dinput8` proxy crate.
3. Add SDK runtime bootstrap and logging.
4. Add class definition types and validation.
5. Add TypeScript host API for `classes.register(...)`.
6. Load the `Super Pirates` test mod.
7. Install the menu class visibility hook.
8. Install the class initialization hook.
9. Verify the end-to-end MVP in the installed game.

## Open Risks

- Barony class UI may require several related hooks instead of one list/count
  hook.
- Class initialization may depend on hardcoded enum ranges beyond the obvious
  `initClass` and `initClassStats` paths.
- Some class IDs may still be serialized as bytes in network or save-adjacent
  code.
- Retail build function layout may differ enough from source that Ghidra work is
  required before even the MVP hooks can be placed safely.
- Anti-virus tooling may flag proxy DLL behavior; distribution docs will need to
  explain the loader clearly.

## Acceptance Criteria

The MVP is successful when:

- Barony starts normally with the SDK loader installed.
- The SDK logs that `Super Pirates` was registered from TypeScript.
- `Super Pirates` appears as a separate playable class in the class menu.
- Selecting `Super Pirates` starts a run.
- The player receives the modded stats and starting weapon.
- Removing the SDK loader restores vanilla Barony behavior.
