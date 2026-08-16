globalvar g
rule "r":
    @Event global
    for g in range(3):
        debug(g)
