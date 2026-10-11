globalvar g
rule "orphan else":
    @Event global
    if g:
    g = 5
    else:
        g = 1
