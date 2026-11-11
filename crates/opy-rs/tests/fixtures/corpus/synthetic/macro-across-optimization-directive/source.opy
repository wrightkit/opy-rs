globalvar g

macro pick():
    if 1 == 1:
        g = 1

#!disableOptimizations
rule "macro body defined before the directive":
    @Event global
    pick()
