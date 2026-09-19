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
| `_`, `__`, `___` translation functions | 🚧 Partial | Translation declarations and supported localized output work; every upstream overload and `.po` lifecycle is not exposed. |

## Member functions and properties

| Source form | Status | Limit |
| --- | --- | --- |
| `array.append`, `array.concat` | ✅ Supported | `append` mutates the receiver; `concat` returns a copy. |
| `array.filter`, `array.map` | ✅ Supported | Element and optional index binders are contextual. |
| `array.all`, `array.any`, `array.unique` | ✅ Supported | `unique` preserves the first occurrence of each value. |
| `array[index]`, `array.slice(start, count)` | ✅ Supported | Indexing and slicing retain their distinct argument contracts. |
| `string.format(...)` | 🚧 Partial | Constant folding and the supported dynamic placeholder forms are compiled; unsupported placeholder shapes produce a source diagnostic. |
| Player members such as `setStatusEffect`, `setMoveSpeed`, `getPosition` and `teleport` | ✅ Supported | Receiver and enum arguments are checked before canonical lowering. |
| `vector.x`, `vector.y`, `vector.z` | ✅ Supported | Property access returns the corresponding numeric component. |
| Member macros using `self` | 🚧 Partial | Supported receiver expansion is limited to the native member-macro forms. |

## Enums, constants and domains

| Source form | Status | Limit |
| --- | --- | --- |
| `Hero`, `Map`, `Gamemode`, `Team`, `Slot`, `Color`, `Button` members | ✅ Supported | Pinned upstream member names, legacy aliases, numeric team/slot filters, and source-only color aliases lower through the canonical Workshop contract; unknown members produce a source diagnostic. |
| `Vector.UP`, `Vector.DOWN`, `Vector.LEFT`, `Vector.RIGHT`, `Vector.FORWARD`, `Vector.BACKWARD` | ✅ Supported | These are canonical vector constants, not arbitrary catalog entries. |
| `Math` numeric, spacing and newline constants | ✅ Supported | `PI`, `E`, `INFINITY`, `EPSILON`, radius multipliers, and the pinned spacing/newline constants lower as numeric or custom-string values. |
| User `enum` declarations and inferred member values | ✅ Supported | Explicit values and inferred increments follow OverPy declaration rules. |
| Contextual reevaluation values such as `ChaseTimeReeval.NONE` and `ChaseRateReeval` | ✅ Supported | Dispatch is determined by the receiving callable's signature. |
| Aliases such as `getCurrentHero`, `hasStatusEffect` and `ChaseReeval` | ✅ Supported | Aliases resolve to their canonical callable or enum identity. |
