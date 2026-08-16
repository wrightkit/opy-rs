globalvar g

rule "r":
    @Event global
    chase(g, 10, 2, ChaseReeval.NONE)
