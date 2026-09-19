# OverPy audited registry notes

The exact leaf inventory for the named upstream registries lives in
[`feature-contracts.json`](feature-contracts.json).
This page provides human-readable compatibility notes; grouped aliases or
notes below must not be used to reconstruct the source set. The upstream registry supplies
the full argument order, argument type/domain, defaults, return behavior and
dispatch rule. The user-facing status is reconciled against current
`opy-rs` evidence.

Reference: `889d9749d1def17f146548cbddb94ea1ab015847`.

## String entities (`src/data/opy/stringEntities.ts`)

The `opyStringEntities` registry defines the user-visible `\\&name;` escape
family. Its exact 67 keys are explicitly classified in the machine inventory;
they remain `❔ Unclassified` / `uncovered` until escape-specific executable
evidence exists. String parsing and escaping behavior remains a separate
contract.

## Keywords (`src/data/opy/keywords.ts`)

| Upstream key | Status |
| --- | --- |
| `and` | ✅ Supported |
| `or` | ✅ Supported |
| `not` | ✅ Supported |
| `bool` | ⚠️ Implemented; leaf evidence partial |
| `float` | ⚠️ Implemented; leaf evidence partial |
| `int` | ⚠️ Implemented; leaf evidence partial |
| `signed` | ⚠️ Implemented; leaf evidence partial |
| `unsigned` | ⚠️ Implemented; leaf evidence partial |
| `case` | ✅ Supported |
| `default` | ✅ Supported |
| `switch` | ✅ Supported |
| `def` | ✅ Supported |
| `subroutine` | ✅ Supported |
| `rule` | ✅ Supported |
| `del` | 🚧 Leaf evidence incomplete |
| `elif` | ✅ Supported |
| `else` | ✅ Supported |
| `if` | ✅ Supported |
| `enum` | ✅ Supported |
| `for` | ✅ Supported |
| `while` | ✅ Supported |
| `globalvar` | ✅ Supported |
| `playervar` | ✅ Supported |
| `goto` | 🚧 Leaf evidence incomplete |
| `loc` | 🚧 Leaf evidence incomplete |
| `in` | ✅ Supported |
| `lambda` | ✅ Supported |
| `macro` | ✅ Supported |
| `self` | 🚧 Leaf evidence incomplete |
| `settings` | 🚧 Leaf evidence incomplete |
| `main` | 🚧 Coming soon |
| `gamemodes` | 🚧 Coming soon |
| `heroes` | 🚧 Coming soon |

## Preprocessing directives (`src/data/opy/preprocessing.ts`)

| Upstream key | Status |
| --- | --- |
| `allowMacroRedeclaration` | ⚠️ Implemented; leaf evidence partial |
| `define` | ⚠️ Implemented; leaf evidence partial |
| `debugElementCount` | ⚠️ Implemented; leaf evidence partial |
| `disableInspector` | ⚠️ Implemented; leaf evidence partial |
| `suppressWarnings` | ⚠️ Implemented; leaf evidence partial |
| `mainFile` | ✅ Supported |
| `include` | ✅ Supported |
| `excludeVariablesInCompilation` | ⚠️ Implemented; leaf evidence partial |
| `setupTags` | ⚠️ Implemented; leaf evidence partial |
| `disableOptimizations` | ⚠️ Implemented; leaf evidence partial |
| `enableOptimizations` | ⚠️ Implemented; leaf evidence partial |
| `optimizeForSize` | ⚠️ Implemented; leaf evidence partial |
| `optimizeForSizeAggressive` | ⚠️ Implemented; leaf evidence partial |
| `disableOptimizeForSize` | ⚠️ Implemented; leaf evidence partial |
| `optimizeStrict` | ⚠️ Implemented; leaf evidence partial |
| `disableOptimizeStrict` | ⚠️ Implemented; leaf evidence partial |
| `replace0ByCapturePercentage` | ⚠️ Implemented; leaf evidence partial |
| `replace0ByPayloadProgressPercentage` | ⚠️ Implemented; leaf evidence partial |
| `replace0ByIsMatchComplete` | ⚠️ Implemented; leaf evidence partial |
| `replace1ByMatchRound` | ⚠️ Implemented; leaf evidence partial |
| `replaceTeam1ByControlScoringTeam` | ⚠️ Implemented; leaf evidence partial |
| `replaceEmptyStringByEmptyArray` | ⚠️ Implemented; leaf evidence partial |
| `replaceEmptyStringByVariable` | ⚠️ Implemented; leaf evidence partial |
| `translations` | ⚠️ Implemented; leaf evidence partial |
| `translateWithPlayerVar` | ⚠️ Implemented; leaf evidence partial |
| `useVariableForCompressionAlphabet` | 🚧 Leaf evidence incomplete |
| `extension` | ⚠️ Implemented; leaf evidence partial |
| `globalvarInitRuleName` | ⚠️ Implemented; leaf evidence partial |
| `playervarInitRuleName` | ⚠️ Implemented; leaf evidence partial |
| `keepUnusedTranslations` | ⚠️ Implemented; leaf evidence partial |
| `disableTranslationSourceLines` | ⚠️ Implemented; leaf evidence partial |
| `writeToOutputFile` | ⚠️ Implemented; leaf evidence partial |
| `postCompileHook` | 🚧 Leaf evidence incomplete |
| `rulePrefix`, `rulePrefixTemplate` | ✅ Supported |

## Annotations (`src/data/opy/annotations.ts`)

| Upstream key | Status | Contract |
| --- | --- | --- |
| `@Name` | ✅ Supported | One string literal; subroutine rule name. |
| `@Event` | ✅ Supported | One event-domain value. |
| `@Team` | ✅ Supported | One team-domain value. |
| `@Slot` | ✅ Supported | One slot-domain value; mutually exclusive with `@Hero`. |
| `@Hero` | ✅ Supported | One hero-domain value; mutually exclusive with `@Slot`. |
| `@Condition` | ✅ Supported | One condition expression; repeatable. |
| `@SuppressWarnings` | ✅ Supported | Space-separated warning names. |
| `@Disabled` | ✅ Supported | No arguments; disables generated rule. |
| `@Delimiter` | ✅ Supported | No arguments; preserves a UI delimiter rule. |
| `@NewPage` | ✅ Supported | No required arguments; inserts page boundary rules. |

## Modules (`src/data/opy/modules.ts`)

| Upstream key | Status | Contract |
| --- | --- | --- |
| `random.randint(min, max)` | ❔ Unclassified | Two inclusive integer bounds; integer result. |
| `random.uniform(min, max)` | ❔ Unclassified | Two float bounds; float result. |
| `random.choice(array)` | ❔ Unclassified | One array; element result. |
| `random.shuffle(array)` | ❔ Unclassified | One array; copied array result. |

## Built-in macros (`src/data/opy/macros.ts`)

| Upstream key | Status |
| --- | --- |
| `buttonToString` | 🚧 Leaf evidence incomplete |
| `.getEffectiveHero`, `.getOppositeTeam` | 🚧 Leaf evidence incomplete |
| `getRealClosestPlayer`, `getRealClosestPlayers` | 🚧 Leaf evidence incomplete |
| `getRealFarthestPlayer`, `getRealFarthestPlayers` | 🚧 Leaf evidence incomplete |
| `.getRealPlayerClosestToReticle`, `.getRealPlayersClosestToReticle` | 🚧 Leaf evidence incomplete |
| `getRealPlayersInRadius`, `.getRealPlayersInViewAngle` | 🚧 Leaf evidence incomplete |
| `getSign`, `getAllPlayers` | 🚧 Leaf evidence incomplete |
| `hudHeader`, `hudSubtext` | ⚠️ Implemented; leaf evidence partial |
| `hudSubheader` | 🚧 Leaf evidence incomplete |
| `lerp`, `lineIntersectsSphere` | 🚧 Leaf evidence incomplete |
| `print`, `.reverse`, `timeToString` | 🚧 Leaf evidence incomplete |
| `.unique` | ✅ Supported; removes duplicate values while preserving first-occurrence order. |

## Built-in functions and member functions

The following table lists every key in `src/data/opy/functions.ts`; entries
beginning with `.` are receiver dispatch entries. The separate
`src/data/opy/memberFunctions.ts` property entries follow it.

| Upstream key | Status |
| --- | --- |
| `_`, `__`, `___` | 🚧 Leaf evidence incomplete |
| `all` | ⚠️ Implemented; leaf evidence partial |
| `any` | ⚠️ Implemented; leaf evidence partial |
| `.append` | ⚠️ Implemented; leaf evidence partial |
| `.all` | ⚠️ Implemented; leaf evidence partial |
| `.any` | ⚠️ Implemented; leaf evidence partial |
| `.filter` | ⚠️ Implemented; leaf evidence partial |
| `.map` | ⚠️ Implemented; leaf evidence partial |
| `.unique` | ✅ Supported |
| `arrayToString` | 🚧 Leaf evidence incomplete |
| `ceil` | ⚠️ Implemented; leaf evidence partial |
| `floor` | ⚠️ Implemented; leaf evidence partial |
| `round` | ⚠️ Implemented; leaf evidence partial |
| `hsl` | 🚧 Leaf evidence incomplete |
| `chaseAtRate` | ⚠️ Implemented; leaf evidence partial |
| `chaseOverTime` | ✅ Supported |
| `compress` | 🚧 Leaf evidence incomplete |
| `compressed` | ⚠️ Implemented; leaf evidence partial |
| `decompressNumbers` | 🚧 Leaf evidence incomplete |
| `decompressVectors` | 🚧 Leaf evidence incomplete |
| `createCasedProgressBarIwt` | 🚧 Leaf evidence incomplete |
| `debug` | 🚧 Leaf evidence incomplete |
| `.format` | 🚧 Leaf evidence incomplete |
| `.remove` | 🚧 Leaf evidence incomplete |
| `getCurrentMap` | ⚠️ Implemented; leaf evidence partial |
| `.getNormal` | 🚧 Leaf evidence incomplete |
| `.getPlayerHit` | 🚧 Leaf evidence incomplete |
| `.getHitPosition` | 🚧 Leaf evidence incomplete |
| `log` | 🚧 Leaf evidence incomplete |
| `pass` | ✅ Supported |
| `range` | ✅ Supported |
| `raycast` | 🚧 Leaf evidence incomplete |
| `ruleCondition` | ⚠️ Implemented; leaf evidence partial |
| `sorted` | ✅ Supported |
| `spacesForString` | 🚧 Leaf evidence incomplete |
| `spacesForLength` | 🚧 Leaf evidence incomplete |
| `strVisualLength` | 🚧 Leaf evidence incomplete |
| `splitDictArray` | 🚧 Leaf evidence incomplete |
| `stopChasing` | 🚧 Leaf evidence incomplete |
| `tabular` | 🚧 Leaf evidence incomplete |
| `.toArray` | 🚧 Leaf evidence incomplete |
| `x` (property member) | ✅ Supported |
| `y` (property member) | ✅ Supported |
| `z` (property member) | ✅ Supported |

The upstream Workshop action/value registries are separate complete data
surfaces in `src/data/actions.ts` and `src/data/values.ts`. They are canonical
Workshop semantics owned by `workshop-rs`; this document does not copy those
registries into `opy-rs`. Their OPY spellings and dispatch contracts are
tracked by the function/member rows above and by the owning Workshop
catalogue.

## Constants (`src/data/opy/constants.ts`)

| Upstream key | Status |
| --- | --- |
| `Vector` (`UP`, `DOWN`, `LEFT`, `RIGHT`, `FORWARD`, `BACKWARD`) | ⚠️ Implemented; leaf evidence partial |
| `Math` (`PI`, `E`, `INFINITY`, `EPSILON`) | 🚧 Leaf evidence incomplete |
| `Texture` (complete texture constant registry) | 🚧 Leaf evidence incomplete |
