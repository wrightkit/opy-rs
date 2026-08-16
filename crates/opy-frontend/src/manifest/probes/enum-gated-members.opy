globalvar g
rule "r":
    @Event eachPlayer
    @Condition eventPlayer.getThrottle() != vect(0, 0, 0)
    @Condition worldVector(vect(1, 2, 3), eventPlayer, Transform.ROTATION) != vect(0, 0, 0)
    eventPlayer.setInvisibility(Invis.ALL)
    eventPlayer.setStatusEffect(eventPlayer, Status.ROOTED, 2)
