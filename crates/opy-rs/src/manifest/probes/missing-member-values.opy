globalvar v

rule "missing member values":
    @Event eachPlayer
    v = eventPlayer.getHorizontalSpeed()
    v = eventPlayer.getVerticalSpeed()
    v = eventPlayer.getNumberOfDeaths()
    v = eventPlayer.getNumberOfElims()
    v = eventPlayer.getNumberOfFinalBlows()
    v = eventPlayer.getScore()
    v = eventPlayer.getPlayerClosestToReticle(Team.ALL)
    v = eventPlayer.isCommunicating(Comms.ACKNOWLEDGE)
    v = eventPlayer.isCommunicatingAnything()
    v = eventPlayer.isCommunicatingSpray()
    v = eventPlayer.isCommunicatingVoiceline()
    v = eventPlayer.isCrouching()
    v = eventPlayer.isInViewAngle(Vector.UP, 30)
    v = eventPlayer.isOnFire()
    v = eventPlayer.isOnWall()
    v = eventPlayer.isStanding()
    v = "abc".strIndex("b")
