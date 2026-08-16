globalvar g
playervar P

rule "chase keyword forms":
    @Event eachPlayer
    chase(g, 10, rate=2, ChaseReeval.NONE)
    chase(g, 10, duration=3, ChaseReeval.NONE)
    chase(eventPlayer.P, 0, rate=1, ChaseReeval.NONE)
    chaseOverTime(g, 10, duration=3)
    chaseOverTime(g, 10, 3, reevaluation=ChaseTimeReeval.NONE)
    wait(time=1)
    g = vect(x=1, y=2, z=3)
    g = len(array=[1, 2])
    print(text="x")
    g = getPlayersInRadius(center=vect(0, 0, 0), radius=10, team=Team.ALL)
    eventPlayer.setStatusEffect(assister=eventPlayer, status=Status.ROOTED, duration=2)
