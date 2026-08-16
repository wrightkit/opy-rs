globalvar g
rule "r":
    @Event eachPlayer
    @Condition eventPlayer.getCurrentHero() != null
    @Condition eventPlayer.hasStatusEffect(Status.BURNING) == false
    disableInspector()
