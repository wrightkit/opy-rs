globalvar g

rule "r":
    @Event global
    chase(10, 10, rate=2, ChaseReeval.NONE)
