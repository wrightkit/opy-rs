use workshop_rs::Value;

const RANDOM_CALLS: [&str; 4] = [
    "randomInteger",
    "randomReal",
    "randomValueInArray",
    "randomizedArray",
];

pub(in crate::compiler) fn same(left: &Value, right: &Value) -> bool {
    compare(left, right, false)
}

pub(in crate::compiler) fn same_shape(left: &Value, right: &Value) -> bool {
    compare(left, right, true)
}

fn compare(left: &Value, right: &Value, shape_only: bool) -> bool {
    match (left, right) {
        (Value::Number(a), Value::Number(b)) => a == b,
        (Value::String(a), Value::String(b))
        | (Value::LocalizedString(a), Value::LocalizedString(b))
        | (Value::GlobalVariable(a), Value::GlobalVariable(b))
        | (Value::Subroutine(a), Value::Subroutine(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Null, Value::Null) | (Value::EventPlayer, Value::EventPlayer) => true,
        (Value::Array(a), Value::Array(b)) => compare_children(a, b, shape_only),
        (Value::Array(elements), Value::Call { name, args })
        | (Value::Call { name, args }, Value::Array(elements))
            if shape_only =>
        {
            let array_name = if elements.is_empty() {
                "emptyArray"
            } else {
                "array"
            };
            name == array_name && compare_children(elements, args, shape_only)
        }
        (
            Value::Vector {
                x: ax,
                y: ay,
                z: az,
            },
            Value::Vector {
                x: bx,
                y: by,
                z: bz,
            },
        ) => {
            compare(ax, bx, shape_only)
                && compare(ay, by, shape_only)
                && compare(az, bz, shape_only)
        }
        (
            Value::Enum {
                value_type: at,
                value: av,
            },
            Value::Enum {
                value_type: bt,
                value: bv,
            },
        ) => at == bt && av == bv,
        (
            Value::PlayerVariable {
                player: ap,
                variable: av,
            },
            Value::PlayerVariable {
                player: bp,
                variable: bv,
            },
        ) => av == bv && compare(ap, bp, shape_only),
        (Value::Call { name: an, args: aa }, Value::Call { name: bn, args: ba }) => {
            an == bn
                && (shape_only || !RANDOM_CALLS.contains(&an.as_str()))
                && compare_children(aa, ba, shape_only)
        }
        _ => false,
    }
}

fn compare_children(left: &[Value], right: &[Value], shape_only: bool) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(a, b)| compare(a, b, shape_only))
}
