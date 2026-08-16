globalvar g

rule "r":
    @Event global
    chaseOverTime(10, 0, 30, ChaseTimeReeval.NONE)
