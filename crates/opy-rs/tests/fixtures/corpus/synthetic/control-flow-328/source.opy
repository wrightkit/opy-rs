globalvar a
globalvar i

rule "continue for":
    @Event global
    for i in range(0, 3):
        if i == 1:
            continue
        i = i + 1

rule "continue while":
    @Event global
    while i < 3:
        if i == 1:
            continue
        i = i + 1

rule "continue do":
    @Event global
    do:
        if i == 1:
            continue
        i = i + 1
    while i < 3

rule "dynamic goto":
    @Event global
    a = 1
    goto loc+a
    a = 2

rule "conditional dynamic goto":
    @Event global
    a = 1
    if a == 1:
        goto loc+a
    a = 3

rule "continue switch in for":
    @Event global
    for i in range(0, 3):
        switch i:
            case 1:
                continue
            default:
                i = i + 1
        a = a + 1

rule "continue switch in while":
    @Event global
    while i < 3:
        switch i:
            case 1:
                continue
            default:
                i = i + 1
        a = a + 1

rule "continue switch in do":
    @Event global
    do:
        switch i:
            case 1:
                continue
            default:
                i = i + 1
        a = a + 1
    while i < 3

rule "rule start":
    @Event global
    a = 1
    goto RULE_START
