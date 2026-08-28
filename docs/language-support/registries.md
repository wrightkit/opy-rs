# OverPy audited inventory: complete source registries

This page is the leaf inventory for the named upstream registries. Every key
in the pinned OverPy `9.7.10` files is listed below; no current `opy-rs`
manifest or fixture is used to define the set. The upstream registry supplies
the full argument order, argument type/domain, defaults, return behavior and
dispatch rule. The user-facing status is reconciled against current
`opy-rs` evidence.

Reference: `889d9749d1def17f146548cbddb94ea1ab015847`.

## Keywords (`src/data/opy/keywords.ts`)

| Upstream key | Status |
| --- | --- |
| `and` | ✅ Supported |
| `or` | ✅ Supported |
| `not` | ✅ Supported |
| `bool` | ✅ Supported |
| `float` | ✅ Supported |
| `int` | ✅ Supported |
| `signed` | ✅ Supported |
| `unsigned` | ✅ Supported |
| `case` | ✅ Supported |
| `default` | ✅ Supported |
| `switch` | ✅ Supported |
| `def` | ✅ Supported |
| `subroutine` | ✅ Supported |
| `rule` | ✅ Supported |
| `del` | 🚧 Coming soon |
| `elif` | ✅ Supported |
| `else` | ✅ Supported |
| `if` | ✅ Supported |
| `enum` | ✅ Supported |
| `for` | ✅ Supported |
| `while` | ✅ Supported |
| `globalvar` | ✅ Supported |
| `playervar` | ✅ Supported |
| `goto` | 🚧 Coming soon |
| `loc` | 🚧 Coming soon |
| `in` | ✅ Supported |
| `lambda` | ✅ Supported |
| `macro` | ✅ Supported |
| `self` | 🚧 Coming soon |
| `settings` | ✅ Bounded compiler slice |
| `main` | 🚧 Coming soon |
| `gamemodes` | 🚧 Coming soon |
| `heroes` | 🚧 Coming soon |

## Preprocessing directives (`src/data/opy/preprocessing.ts`)

| Upstream key | Status |
| --- | --- |
| `allowMacroRedeclaration` | 🚧 Coming soon |
| `define` | ✅ Supported |
| `debugElementCount` | 🚧 Coming soon |
| `disableInspector` | 🚧 Coming soon |
| `suppressWarnings` | 🚧 Coming soon |
| `mainFile` | ✅ Supported |
| `include` | ✅ Supported |
| `excludeVariablesInCompilation` | 🚧 Coming soon |
| `setupTags` | 🚧 Coming soon |
| `disableOptimizations` | 🚧 Coming soon |
| `enableOptimizations` | 🚧 Coming soon |
| `optimizeForSize` | 🚧 Coming soon |
| `optimizeForSizeAggressive` | 🚧 Coming soon |
| `disableOptimizeForSize` | 🚧 Coming soon |
| `optimizeStrict` | 🚧 Coming soon |
| `disableOptimizeStrict` | 🚧 Coming soon |
| `replace0ByCapturePercentage` | 🚧 Coming soon |
| `replace0ByPayloadProgressPercentage` | 🚧 Coming soon |
| `replace0ByIsMatchComplete` | 🚧 Coming soon |
| `replace1ByMatchRound` | 🚧 Coming soon |
| `replaceTeam1ByControlScoringTeam` | 🚧 Coming soon |
| `replaceEmptyStringByEmptyArray` | 🚧 Coming soon |
| `replaceEmptyStringByVariable` | 🚧 Coming soon |
| `translations` | 🚧 Coming soon |
| `translateWithPlayerVar` | 🚧 Coming soon |
| `useVariableForCompressionAlphabet` | 🚧 Coming soon |
| `extension` | 🚧 Coming soon |
| `globalvarInitRuleName` | 🚧 Coming soon |
| `playervarInitRuleName` | 🚧 Coming soon |
| `keepUnusedTranslations` | 🚧 Coming soon |
| `disableTranslationSourceLines` | 🚧 Coming soon |
| `writeToOutputFile` | 🚧 Coming soon |
| `postCompileHook` | ✅ Bounded compiler slice |
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
| `random.randint(min, max)` | 🚧 Coming soon | Two inclusive integer bounds; integer result. |
| `random.uniform(min, max)` | 🚧 Coming soon | Two float bounds; float result. |
| `random.choice(array)` | 🚧 Coming soon | One array; element result. |
| `random.shuffle(array)` | 🚧 Coming soon | One array; copied array result. |

## Built-in macros (`src/data/opy/macros.ts`)

| Upstream key | Status |
| --- | --- |
| `buttonToString` | 🚧 Coming soon |
| `.getEffectiveHero`, `.getOppositeTeam` | 🚧 Coming soon |
| `getRealClosestPlayer`, `getRealClosestPlayers` | 🚧 Coming soon |
| `getRealFarthestPlayer`, `getRealFarthestPlayers` | 🚧 Coming soon |
| `.getRealPlayerClosestToReticle`, `.getRealPlayersClosestToReticle` | 🚧 Coming soon |
| `getRealPlayersInRadius`, `.getRealPlayersInViewAngle` | 🚧 Coming soon |
| `getSign`, `getAllPlayers` | 🚧 Coming soon |
| `hudHeader`, `hudSubheader`, `hudSubtext` | 🚧 Coming soon |
| `lerp`, `lineIntersectsSphere` | 🚧 Coming soon |
| `print`, `.reverse`, `timeToString`, `.unique` | 🚧 Coming soon |

## Built-in functions and member functions

The following table lists every key in `src/data/opy/functions.ts`; entries
beginning with `.` are receiver dispatch entries. The separate
`src/data/opy/memberFunctions.ts` property entries follow it.

| Upstream key | Status |
| --- | --- |
| `_`, `__`, `___` | 🚧 Coming soon |
| `all` | ✅ Supported |
| `any` | ✅ Supported |
| `.append` | ✅ Supported |
| `.all` | ✅ Supported |
| `.any` | ✅ Supported |
| `.filter` | ✅ Supported |
| `.map` | ✅ Supported |
| `arrayToString` | 🚧 Coming soon |
| `ceil` | 🚧 Coming soon |
| `floor` | 🚧 Coming soon |
| `round` | 🚧 Coming soon |
| `hsl` | 🚧 Coming soon |
| `chaseAtRate` | 🚧 Coming soon |
| `chaseOverTime` | 🚧 Coming soon |
| `compress` | 🚧 Coming soon |
| `compressed` | 🚧 Coming soon |
| `decompressNumbers` | 🚧 Coming soon |
| `decompressVectors` | 🚧 Coming soon |
| `createCasedProgressBarIwt` | 🚧 Coming soon |
| `debug` | 🚧 Coming soon |
| `.format` | 🚧 Coming soon |
| `.remove` | 🚧 Coming soon |
| `getCurrentMap` | ✅ Supported |
| `.getNormal` | 🚧 Coming soon |
| `.getPlayerHit` | 🚧 Coming soon |
| `.getHitPosition` | 🚧 Coming soon |
| `log` | 🚧 Coming soon |
| `pass` | ✅ Supported |
| `range` | ✅ Supported |
| `raycast` | 🚧 Coming soon |
| `ruleCondition` | ✅ Supported |
| `sorted` | ✅ Supported |
| `spacesForString` | 🚧 Coming soon |
| `spacesForLength` | 🚧 Coming soon |
| `strVisualLength` | 🚧 Coming soon |
| `splitDictArray` | 🚧 Coming soon |
| `stopChasing` | 🚧 Coming soon |
| `tabular` | 🚧 Coming soon |
| `.toArray` | 🚧 Coming soon |
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
| `Vector` (`UP`, `DOWN`, `LEFT`, `RIGHT`, `FORWARD`, `BACKWARD`) | ✅ Supported |
| `Math` (`PI`, `E`, `INFINITY`, `EPSILON`, and documented spacing/radius constants) | 🚧 Coming soon |
| `Texture` (complete texture constant registry) | 🚧 Coming soon |
