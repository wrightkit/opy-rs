globalvar value

rule "issue 28 syntax":
    @Event global
    do:
        value = 1
    while value not in [2, 3]
    switch value:
        case 0x10:
            value = 1 in [1, 2]
        case 0X20:
            value = [item * 2 for item in [1, 2]]
        default:
            value = {"key": 1}["key"]
    value = sorted([1, 2], key=lambda item: item)
    value = f"formatted {0}"
    value = w"wide"
