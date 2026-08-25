globalvar value = 0

rule "issue 47 control flow":
    @Event global
    do:
        value += 1
        if value == 1:
            break
        value += 10
    while value < 2
    if value == 2:
        value = 3
    elif value == 4:
        value = 5
    else:
        value = 6
    for I in range(0, 2):
        if I == 1:
            value += 1
    while value < 5:
        value += 1
    switch value:
        case 5:
            value = 6
        case 6:
            value = 7
            break
        default:
            value = 8
