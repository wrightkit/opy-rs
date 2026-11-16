#!disableOptimizations
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

rule "false while":
    @Event global
    while 1 == 2:
        g = 1
        wait(1)
