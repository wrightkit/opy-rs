globalvar g
rule "r":
    @Event global
    @Condition abs(-2) > 0
    @Condition sqrt(4) > 0
    @Condition len(getAllPlayers()) >= 0
    @Condition random.uniform(0, 1) >= 0
    @Condition len(random.choice([[1]])) >= 0
    @Condition vect(1, 2, 3) != vect(0, 0, 0)
    disableInspector()
    debug(len([1]))
    print("x")
    wait()
    wait(0.5)
    playEffect(getAllPlayers(), DynamicEffect.BAD_EXPLOSION, Color.YELLOW, g, 1)
    createBeam(getAllPlayers(), Beam.GOOD, g, g, Color.YELLOW, EffectReeval.VISIBILITY)
