globalvar g

rule "defaults":
    @Event eachPlayer
    kill(eventPlayer)
    damage(player=eventPlayer, amount=10)
    heal(eventPlayer, null, 5)
    createEffect(getAllPlayers(), Effect.GOOD_AURA, Color.YELLOW, eventPlayer, 3)
    startHealingModification(getAllPlayers(), getAllPlayers(), 50)
    setObjectiveDescription(text="obj")
    createInWorldText(getAllPlayers(), "hi", vect(0, 0, 0), 2)
    progressBarHud(eventPlayer, 5)
    createProgressBarInWorldText(position=vect(0, 0, 0))
    hudSubheader(text="hi")
    hudSubheader(getAllPlayers(), "hi", location=HudPosition.LEFT, specVisibility=SpecVisibility.DEFAULT)
    hudText()
    createDummy(Hero.ANA, Team.ALL, 0, vect(0, 0, 0))
    createBeam(eventPlayer, Beam.GOOD, vect(0, 0, 0), vect(0, 0, 0))
    g = getPlayersInSlot(1)
    g = getPlayersOnObjective()
    g = getNumberOfSlots()
    g = getRealPlayersInRadius(vect(0, 0, 0), 3)
