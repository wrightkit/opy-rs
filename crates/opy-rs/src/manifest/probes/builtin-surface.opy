globalvar g

rule "builtin surface":
    @Event eachPlayer
    @Condition isAssemblingHeroes() == false
    @Condition isInSetup() == false
    @Condition horizontalAngleFromDirection(vect(1, 0, 0)) == 90
    @Condition eventPlayer.isHoldingButton(Button.INTERACT)
    @Condition eventPlayer.getEyePosition() == vect(0, 0, 0)
    @Condition horizontalAngleFromDirection(eventPlayer.getFacingDirection()) == 90
    @Condition any([g > 0 for g in [1, 2]])
    @Condition all([g > 0 for g in [1, 2]])
    @Condition eventPlayer.getHero() == Hero.ANA
    @Condition eventPlayer.hasStatus(Status.ROOTED)
    @Condition distance(eventPlayer.getPosition(), vect(0, 0, 0)) < 2
    @Condition strContains("abc", "b")
    @Condition ceil(1.2) == 2
    @Condition floor(1.8) == 1
    @Condition round(1.5) == 2
    g = getPlayers(Team.ALL)
    g = getAllHeroes()
    g = heroIcon(Hero.ANA)
    g = getMatchTime()
    g = angleToDirection(30, 0)
    g = buttonString(Button.INTERACT)
    g = random.randint(1, 3)
    g = random.shuffle([1, 2])
    g = g.concat([1])
    g = g.exclude(1)
    g = vectorTowards(vect(0, 0, 0), vect(1, 0, 0))
    g = dotProduct(vect(1, 0, 0), vect(1, 0, 0))
    g = getServerLoad()
    g = getAverageServerLoad()
    g = getPeakServerLoad()
    g = evalOnce(g)
    g = abilityIconString(Hero.ANA, Button.ABILITY_1)
    g = updateEveryTick(g)
    g = sinDeg(g)
    g = cosDeg(g)
    g = rgb(1, 2, 3)
    g = eventPlayer.getSlot()
    g = eventPlayer.getMaxHealth()
    g = eventPlayer.getUltCharge()
    eventPlayer.addToScore(1)
    g = max(1, 2)
    g = getLastCreatedEntity()
    g = worldVector(Vector.LEFT, eventPlayer, Transform.ROTATION)
    g = worldVector(Vector.LEFT, eventPlayer, Transform.ROTATION).x
    g = worldVector(Vector.LEFT, eventPlayer, Transform.ROTATION).y
    g = worldVector(Vector.LEFT, eventPlayer, Transform.ROTATION).z
    g = "abc".charAt(0)
    g = [1, 2].last()
    chaseAtRate(g, 1, 1)
    createDummy(Hero.ANA, Team.ALL, -1, vect(0, 0, 0), vect(0, 0, 0))
    hudHeader(getAllPlayers(), "header", HudPosition.TOP, 0, Color.WHITE, HudReeval.VISIBILITY, SpecVisibility.DEFAULT)
    hudHeader(getAllPlayers(), "always", HudPosition.TOP, 1, Color.WHITE, HudReeval.VISIBILITY, SpecVisibility.ALWAYS)
    hudSubtext(getAllPlayers(), "text", HudPosition.TOP, 1, Color.WHITE, HudReeval.VISIBILITY, SpecVisibility.DEFAULT)
    hudText(getAllPlayers(), "header", "subheader", "text", HudPosition.TOP, 1, Color.WHITE, Color.WHITE, Color.WHITE, HudReeval.VISIBILITY, SpecVisibility.DEFAULT)
    hudHeader(text="default header")
    hudSubtext(text="default text")
    setMatchTime(0)
    eventPlayer.startFacing(vect(0, 0, 1), 1, Relativity.TO_WORLD, FacingReeval.DIRECTION_AND_TURN_RATE)
    eventPlayer.startForcingHero(Hero.ANA)
    eventPlayer.disallowButton(Button.ULTIMATE)
    eventPlayer.allowButton(Button.ULTIMATE)
    eventPlayer.startHoT(null, 9999, 9999)
    eventPlayer.attachTo(eventPlayer, vect(0, 0, 0))
    eventPlayer.disableEnvironmentCollision(true)
    eventPlayer.setAbility1Enabled(false)
    eventPlayer.setAbility2Enabled(false)
    eventPlayer.setUltEnabled(false)
    eventPlayer.setPrimaryFireEnabled(false)
    eventPlayer.setSecondaryFireEnabled(false)
    eventPlayer.clearStatusEffect(Status.ROOTED)
    destroyAllEffects()
    destroyAllDummies()
    setObjectiveDescription(getAllPlayers(), "objective", HudReeval.VISIBILITY)
    eventPlayer.setFacing(vect(0, 0, 1), Relativity.TO_WORLD)
    createEffect(getAllPlayers(), Effect.BAD_AURA, Color.GREEN, vect(0, 0, 0), 2, EffectReeval.VISIBILITY_POSITION_AND_RADIUS)
    createInWorldText(getAllPlayers(), "header", vect(0, 0, 0), 1, Clip.NONE, WorldTextReeval.VISIBILITY, Color.WHITE, SpecVisibility.DEFAULT)
    eventPlayer.startForcingButton(Button.SECONDARY_FIRE)
    eventPlayer.stopForcingButton(Button.SECONDARY_FIRE)
    eventPlayer.forceButtonPress(Button.ABILITY_1)
    eventPlayer.stopFacing()
    eventPlayer.stopForcingCurrentHero()
    destroyAllHudTexts()
    disableGamemodeCompletion()
    disableScoring()
    eventPlayer.disableKillFeed()
    eventPlayer.startForcingHero(Hero.MCCREE)
    eventPlayer.startForcingHero(Hero.HAMMOND)
    declareTeamVictory(Team.1)
    declarePlayerVictory(eventPlayer)
    setSlowMotion(10)
    stopChasingVariable(g)
    bigMessage(getAllPlayers(), "big")
    smallMessage(getAllPlayers(), "small")
    eventPlayer.disableHeroHUD()
    eventPlayer.enableHeroHud()
    waitUntil(true, 1)

rule "event ability":
    @Event playerDealtDamage
    @Condition eventAbility == Button.PRIMARY_FIRE
    @Condition eventDamage > 0
    eventPlayer.startForcingPosition(vect(0, 0, 0), false)
