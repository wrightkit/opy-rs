globalvar g
rule "r":
    @Event global
    g = wait(1)
