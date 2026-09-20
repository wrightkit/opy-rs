# Functions, member functions and domains

The compiler resolves callable names, receiver types, argument order,
defaults, contextual enum values and return forms before lowering to the
canonical Workshop model.

## Standalone functions

| Source form | Status | Limit |
| --- | --- | --- |
| `abs(value)`, `len(value)`, `range(...)` | ✅ Supported | `range` supports its documented one-, two- and three-argument forms. |
| `wait(duration[, reevaluation])` | ✅ Supported | The omitted reevaluation uses the OverPy default. |
| `raiseToPower(base, exponent)` | ✅ Supported | Numeric arguments are lowered through the Workshop value model. |
| `sorted(array[, key])` | ✅ Supported | The supported key form uses contextual element/index binders. |
| `all(array)`, `any(array)` | ✅ Supported | The array form is supported. |
| `random.randint`, `random.uniform`, `random.choice`, `random.shuffle` | ✅ Supported | Argument domains and copied-array behavior follow OverPy. |
| `_`, `__`, `___` translation functions | ✅ Supported | One-argument and literal-context two-argument forms lower through the existing translation helper; external `.po` lifecycle remains outside this repository. |
| Pinned built-in macro helpers (`buttonToString`, `getReal*`, `getSign`, `lerp`, HUD helpers, `timeToString`) | ✅ Supported | Source callables resolve through typed special lowering; canonical Workshop identities remain in the Workshop catalog. |

## Member functions and properties

| Source form | Status | Limit |
| --- | --- | --- |
| `array.append`, `array.concat` | ✅ Supported | `append` mutates the receiver; `concat` returns a copy. |
| `array.filter`, `array.map` | ✅ Supported | Element and optional index binders are contextual. |
| `array.all`, `array.any`, `array.unique` | ✅ Supported | `unique` preserves the first occurrence of each value. |
| `array[index]`, `array.slice(start, count)` | ✅ Supported | Indexing and slicing retain their distinct argument contracts. |
| `string.format(...)` | ✅ Supported | Constant and dynamic indexed/sequential placeholder forms lower to canonical custom strings, including chunked forms beyond three dynamic values. |
| Player members such as `setStatusEffect`, `setMoveSpeed`, `getPosition` and `teleport` | ✅ Supported | Receiver and enum arguments are checked before canonical lowering. |
| `vector.x`, `vector.y`, `vector.z` | ✅ Supported | Property access returns the corresponding numeric component. |
| Member macros using `self` | ✅ Supported | Class-qualified member macros expand `self` to the call receiver while preserving argument and statement boundaries. |

## Enums, constants and domains

| Source form | Status | Limit |
| --- | --- | --- |
| `Hero`, `Map`, `Gamemode`, `Team`, `Slot`, `Color`, `Button`, `Texture` members | ✅ Supported | Pinned upstream member names, legacy aliases, numeric team/slot filters, source-only color aliases, and texture-tag setup lower through the canonical Workshop contract; unknown members produce a source diagnostic. |
| `Vector.UP`, `Vector.DOWN`, `Vector.LEFT`, `Vector.RIGHT`, `Vector.FORWARD`, `Vector.BACKWARD` | ✅ Supported | These are canonical vector constants, not arbitrary catalog entries. |
| `Math` numeric, spacing and newline constants | ✅ Supported | `PI`, `E`, `INFINITY`, `EPSILON`, radius multipliers, and the pinned spacing/newline constants lower as numeric or custom-string values. |
| User `enum` declarations and inferred member values | ✅ Supported | Explicit values and inferred increments follow OverPy declaration rules. |
| Contextual reevaluation values such as `ChaseTimeReeval.NONE` and `ChaseRateReeval` | ✅ Supported | Dispatch is determined by the receiving callable's signature. |
| Aliases such as `getCurrentHero`, `hasStatusEffect` and `ChaseReeval` | ✅ Supported | Aliases resolve to their canonical callable or enum identity. |
