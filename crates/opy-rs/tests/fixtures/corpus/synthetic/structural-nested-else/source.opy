#!optimizeForSize
globalvar g
globalvar h
rule "x":
    @Event global
    if g == 1:
        g = 2
        if h == null or h == 0:
            h = 3
    else:
        if h == null or h == 0:
            h = 4
    g = 5
