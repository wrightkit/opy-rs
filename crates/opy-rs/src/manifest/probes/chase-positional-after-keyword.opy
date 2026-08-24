globalvar g

rule "r":
    @Event global
    chaseOverTime(g, duration=3, 5)
