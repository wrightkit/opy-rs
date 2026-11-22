globalvar a

rule "pass inside relative jump":
    @Event global
    a = 1
    goto loc+2
    pass
    a = 2
    a = 3

rule "pass past relative jump":
    @Event global
    goto loc+2
    a = 1
    a = 2
    pass
    a = 3

rule "pass inside while after relative jump":
    @Event global
    goto loc+1
    a = 1
    while a < 5:
        pass
    a = 2

rule "pass inside if after relative jump":
    @Event global
    goto loc+3
    if a == 1:
        pass
    a = 2

rule "pass after conditional relative jump":
    @Event global
    if a == 1:
        goto loc+a
    pass
    a = 2

rule "pass after skip if":
    @Event global
    __skipIf__(a == 1, 2)
    pass
    a = 2
    a = 3

rule "pass before jump":
    @Event global
    a = 1
    pass
    goto loc+2
    a = 2
    a = 3

rule "pass without jump":
    @Event global
    a = 1
    pass
    a = 2

rule "pass after label jump":
    @Event global
    a = 1
    goto target
    pass
    a = 2
    target:
    a = 3

rule "pass after abort if":
    @Event global
    __abortIf__(a == 1)
    pass
    a = 2

rule "pass after loop if":
    @Event global
    __loopIf__(a == 1)
    pass
    a = 2

rule "pass after jump inside nested branch":
    @Event global
    if a == 1:
        goto loc+2
        pass
        a = 2
    pass
    a = 3

rule "pass after jump inside loop":
    @Event global
    while a < 5:
        goto loc+2
        pass
        a = 2
    pass
    a = 3

rule "constant if after relative jump":
    @Event global
    goto loc+2
    if false:
        pass
    a = 2

rule "constant while after relative jump":
    @Event global
    goto loc+2
    while false:
        pass
    a = 2

rule "pass in switch after relative jump":
    @Event global
    goto loc+2
    switch a:
        case 1:
            pass
    a = 2

rule "pass after rule start":
    @Event global
    goto RULE_START
    pass
    a = 2

rule "pass state resets between rules":
    @Event global
    a = 1
    pass
    a = 2
