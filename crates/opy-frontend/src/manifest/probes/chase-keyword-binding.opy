globalvar g

rule "chase keyword binding":
    @Event eachPlayer
    chaseOverTime(g, 10, duration=3)
    chaseOverTime(g, 10, 3, reevaluation=ChaseTimeReeval.NONE)
    wait(time=1)
    wait(waitBehavior=Wait.IGNORE_CONDITION, time=2)
    print(text="x")
    g = len(array=[1, 2])
    g = vect(x=1, y=2, z=3)
    eventPlayer.setStatusEffect(assister=eventPlayer, status=Status.ROOTED, duration=2)
    g = getPlayersInRadius(center=vect(0, 0, 0), radius=10, team=Team.ALL)
