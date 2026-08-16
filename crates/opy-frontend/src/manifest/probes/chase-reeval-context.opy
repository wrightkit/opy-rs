globalvar g

rule "chase reevaluation domain selection":
    @Event global
    chase(g, 10, rate=2, ChaseReeval.DESTINATION_AND_RATE)
    chase(g, 10, duration=3, ChaseReeval.DESTINATION_AND_DURATION)
