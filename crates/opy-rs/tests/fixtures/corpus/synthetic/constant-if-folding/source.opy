globalvar g
globalvar h
globalvar A

rule "true if":
    @Event global
    if 1 == 1:
        g = 1

rule "false if":
    @Event global
    if 1 + 1 == 3:
        g = 4

rule "true if else":
    @Event global
    if 1 == 1:
        g = 1
    else:
        g = 2

rule "false if else":
    @Event global
    if 1 == 2:
        g = 1
    else:
        g = 2

rule "false elif":
    @Event global
    if A == 1:
        g = 1
    elif 1 == 2:
        g = 2
    else:
        g = 3

rule "true elif":
    @Event global
    if A == 1:
        g = 1
    elif 1 == 1:
        g = 2
    elif A == 2:
        g = 3
    else:
        g = 4

rule "false if true elif":
    @Event global
    if 1 == 2:
        g = 1
    elif 1 == 1:
        g = 2
    else:
        g = 3

rule "false if live elif":
    @Event global
    if 1 == 2:
        g = 1
    elif A == 2:
        g = 2
    else:
        g = 3

rule "nested":
    @Event global
    if A == 1:
        if 1 == 1:
            g = 1
        if 1 == 2:
            g = 2
        h = 3

rule "in while":
    @Event global
    while A == 1:
        if 1 == 1:
            g = 1
        wait(1)

rule "false while":
    @Event global
    while 1 == 2:
        g = 1
        wait(1)

rule "live condition":
    @Event global
    if A == 1 and 1 == 1:
        g = 1
