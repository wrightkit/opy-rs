globalvar g

rule "r":
    @Event global
    chaseOverTime(g, 10, bogus=1)
