globalvar g
playervar p

rule "action rewrites":
    @Event eachPlayer
    hudText(eventPlayer, "", "", "", HudPosition.LEFT, 3, Color.YELLOW, Color.YELLOW, Color.YELLOW, HudReeval.VISIBILITY_AND_STRING, SpecVisibility.DEFAULT)
    hudText(eventPlayer, "", g, "", HudPosition.LEFT, 3, Color.YELLOW, Color.YELLOW, Color.YELLOW, HudReeval.VISIBILITY_AND_STRING, SpecVisibility.DEFAULT)
    eventPlayer.setUltCharge(0.5)
    eventPlayer.setUltCharge(50)
    eventPlayer.setUltCharge(-2.5)
    waitUntil(null, 3)
    waitUntil(getLivingPlayers(Team.ALL), 3)
    waitUntil(getAllPlayers(), 3)
    waitUntil(g, 3)
    waitUntil(vect(1, 2, 3), 3)
    waitUntil(Vector.UP, 3)
    waitUntil(Hero.ANA, 3)
    waitUntil(Team.1, 3)
    waitUntil("abc", 3)
    waitUntil(eventPlayer.getPosition(), 3)
    waitUntil(eventPlayer.getTeam(), 3)
    eventPlayer.setAbility1Enabled(null)
    eventPlayer.addHealthPool(Health.ARMOR, 3, true, null)
    eventPlayer.startForcingOutlineFor(eventPlayer, false, Color.RED, OutlineVisibility.ALWAYS)
    eventPlayer.startForcingOutlineFor(eventPlayer, true, Color.RED, OutlineVisibility.ALWAYS)
