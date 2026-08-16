globalvar g
playervar P

rule "chase keyword forms":
    @Event eachPlayer
    chase(g, 10, rate=2, ChaseReeval.NONE)
    chase(g, 10, duration=3, ChaseReeval.NONE)
    chase(eventPlayer.P, 0, rate=1, ChaseReeval.NONE)
    chase(eventPlayer.P, 0, duration=1, ChaseReeval.DESTINATION_AND_DURATION)
