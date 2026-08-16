globalvar g

rule "r":
    @Event global
    chase(g, 10, rate=2, ChaseReeval.DESTINATION_AND_DURATION)
