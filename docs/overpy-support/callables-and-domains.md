# OverPy audited inventory: callables and domains

Source: pinned OverPy `9.7.10`, content commit
`889d9749d1def17f146548cbddb94ea1ab015847`. The external callable registries
are `src/data/opy/functions.ts`, `memberFunctions.ts`, `constants.ts`,
`modules.ts`, and `macros.ts`; Workshop registries are `src/data/actions.ts`,
`values.ts`, `constants.ts`, `heroes.ts`, `maps.ts`, `gamemodes.ts`,
`localizedStrings.ts`, and `customGameSettings.ts`.

The upstream registries are the audited inventory source and are not copied
into `opy-rs`. Each callable contract has a spelling, receiver (if any),
ordered arguments, argument type/domain, optional/default behavior, return
behavior, and dispatch rule.

## Standalone functions and operators

| Feature / representative leaf | Status | Audited contract |
| --- | --- | --- |
| `abs(value)` | ✅ Supported | One numeric value; numeric result. |
| `len(arrayOrString)` | ✅ Supported | One array/string value; integer result. |
| `range(stop)` / `range(start, stop[, step])` | ✅ Supported | Optional start and step have distinct defaults. |
| `wait(duration[, reevaluation])` | ✅ Supported | Reevaluation has an optional default. |
| `raiseToPower(base, exponent)` | ✅ Supported | Two numeric arguments in order; value operation. |
| `sorted(array[, key])` | ✅ Supported | Optional lambda key; element/index binder is contextual. |
| `all(array)` / `any(array)` | ✅ Supported | One boolean-array value. |
| `ceil(value)` / `floor(value)` / `round(value)` | ✅ Supported | Numeric rounding maps to the canonical `Rounding` domain. |
| `random.randint(min, max)` | ✅ Supported | Two inclusive integer bounds; integer result. |
| `random.uniform(min, max)` | ✅ Supported | Two float bounds; float result. |
| `random.choice(array)` | ✅ Supported | One array; returns an element or supplied non-array value. |
| `random.shuffle(array)` | ✅ Supported | One array; returns a copied array. |
| `_(contextOrString[, string])` | 🚧 Coming soon | One-argument and two-argument modes differ. |

## Receiver/member functions

| Feature / representative leaf | Status | Audited contract |
| --- | --- | --- |
| `array.append(value)` | ✅ Supported | Array receiver; mutating; arrays are extended. |
| `array.concat(value)` | ✅ Supported | Array receiver; returns a copy. |
| `array.filter(lambda)` | ✅ Supported | Lambda result selects elements; optional index binder. |
| `array.map(lambda)` | ✅ Supported | Lambda result replaces each element. |
| `array.all([lambda])` / `array.any([lambda])` | ✅ Supported | Optional lambda defaults to element truthiness. |
| `array[index]` and `array.slice(start, count)` | ✅ Supported | Indexing and slicing have different arguments. |
| `string.format(...)` | 🚧 Coming soon | Variadic formatting remains incomplete. |
| `player.setStatusEffect(player, assister, status, duration)` | 🚧 Coming soon | Receiver plus four ordered explicit arguments. |
| `vector.x`, `.y`, `.z` | ✅ Supported | Property-like vector access; numeric result. |
| `self` in member macros | 🚧 Coming soon | Dispatch target is the macro receiver. |

## Constants, enums and contextual dispatch

| Feature | Status | Notes |
| --- | --- | --- |
| `Hero`, `Map`, `Gamemode`, `Team`, `Slot`, `Color`, `Button` domains | 🚧 Coming soon | Membership and spelling are domain-specific. |
| `Vector.UP/DOWN/LEFT/RIGHT/FORWARD/BACKWARD` | ✅ Supported | Constants are separate from arbitrary vectors. |
| `Math.PI`, `Math.E`, `Math.INFINITY`, `Math.EPSILON` | 🚧 Coming soon | Numeric constants are distinct leaves. |
| User enum assignment and inferred increments | ✅ Supported | Separate from Workshop catalog domains. |
| Contextual `None`/reevaluation enum dispatch | 🚧 Coming soon | `ChaseTimeReeval`, `ChaseRateReeval` and `Invis` differ. |
| Alias resolution (`getCurrentHero`, `hasStatusEffect`, `ChaseReeval`) | ✅ Supported | Non-contextual and call-context aliases differ. |

The pinned `functions.ts`, `actions.ts`, `memberFunctions.ts` and `values.ts`
registries contain the complete callable surface. The audit keeps families
separate because action/value position, receiver type, defaults, overloads and
return behavior differ. A name in the current internal manifest is evidence
for `opy-rs` only; it does not expand this inventory or make an incomplete
callable green.
