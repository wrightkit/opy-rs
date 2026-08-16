globalvar g
rule "r":
    @Event global
    g = frobnicate()
