globalvar g

rule "builtin surface":
    @Event eachPlayer
    @Condition isAssemblingHeroes() == false
    @Condition horizontalAngleFromDirection(vect(1, 0, 0)) == 90
    @Condition eventPlayer.isHoldingButton(Button.INTERACT)
    @Condition horizontalAngleFromDirection(eventPlayer.getFacingDirection()) == 90
    @Condition any([g > 0 for g in [1, 2]])
    @Condition all([g > 0 for g in [1, 2]])
    @Condition ceil(1.2) == 2
    @Condition floor(1.8) == 1
    @Condition round(1.5) == 2
    g = getPlayers(Team.ALL)
    g = angleToDirection(30, 0)
    g = buttonString(Button.INTERACT)
    g = random.randint(1, 3)
    g = random.shuffle([1, 2])
    chaseAtRate(g, 1, 1)
    createDummy(Hero.ANA, Team.ALL, -1, vect(0, 0, 0), vect(0, 0, 0))
    hudHeader(getAllPlayers(), "header", HudPosition.TOP, 0, Color.WHITE, HudReeval.VISIBILITY, SpecVisibility.DEFAULT)
    hudSubtext(getAllPlayers(), "text", HudPosition.TOP, 1, Color.WHITE, HudReeval.VISIBILITY, SpecVisibility.DEFAULT)
    setMatchTime(0)
    eventPlayer.startFacing(vect(0, 0, 1), 1, Relativity.TO_WORLD, FacingReeval.DIRECTION_AND_TURN_RATE)
    eventPlayer.disableHeroHUD()
    eventPlayer.enableHeroHud()
    waitUntil(true, 1)
