globalvar g
globalvar h
rule "x":
    @Event global
    g = 1
    if h == null:
        goto lbl_0
    lbl_0:
    g = 2
rule "y":
    @Event global
    g = 1
    if h == null:
        goto lbl_1
    g = 5
    lbl_1:
    g = 2
rule "z":
    @Event global
    g = 1
    if h == null:
        goto lbl_2
    g = 5
    g = 6
    lbl_2:
